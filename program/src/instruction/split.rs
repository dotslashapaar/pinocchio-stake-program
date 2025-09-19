use crate::{
    error::*, helpers::*, state::accounts::StakeAuthorize, state::stake_state_v2::StakeStateV2,
    state::StakeHistorySysvar,
};
use pinocchio::{
    account_info::AccountInfo,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::{clock::Clock, Sysvar},
    ProgramResult,
};

pub fn process_split(accounts: &[AccountInfo], split_lamports: u64) -> ProgramResult {
    msg!("Split: begin");
    let mut arr_of_signers = [Pubkey::default(); MAXIMUM_SIGNERS];
    let _ = collect_signers(accounts, &mut arr_of_signers)?;

    let [source_stake_account_info, destination_stake_account_info, _] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    msg!("Split: destructured accounts");
    // Trace key account flags
    if source_stake_account_info.is_signer() { msg!("Split: src signer=1"); } else { msg!("Split: src signer=0"); }
    if source_stake_account_info.is_writable() { msg!("Split: src writable=1"); } else { msg!("Split: src writable=0"); }
    if destination_stake_account_info.is_signer() { msg!("Split: dst signer=1"); } else { msg!("Split: dst signer=0"); }
    if destination_stake_account_info.is_writable() { msg!("Split: dst writable=1"); } else { msg!("Split: dst writable=0"); }
    if *source_stake_account_info.owner() == crate::ID { msg!("Split: src owner ok"); } else { msg!("Split: src owner mismatch"); return Err(ProgramError::InvalidAccountOwner); }
    if *destination_stake_account_info.owner() == crate::ID { msg!("Split: dst owner ok"); } else { msg!("Split: dst owner mismatch"); return Err(ProgramError::InvalidAccountOwner); }


    let clock = Clock::get()?;
    msg!("Split: got Clock");
    let stake_history = &StakeHistorySysvar(clock.epoch);

    let source_data_len = source_stake_account_info.data_len();
    let destination_data_len = destination_stake_account_info.data_len();
    if source_data_len == 0 { msg!("Split: src len=0"); }
    if destination_data_len == 0 { msg!("Split: dest len=0"); }
    let min = StakeStateV2::size_of();
    if destination_data_len == 0 { msg!("Split: dest len=0"); }
    else if destination_data_len < min { msg!("Split: dest len<min"); }
    else { msg!("Split: dest len>=min"); }
    if destination_data_len < StakeStateV2::size_of() {
        msg!("Split: dest size too small");
        return Err(ProgramError::InvalidAccountData);
    }

    // Be tolerant of account data alignment for destination Uninitialized check.
    // Only require that the destination deserializes to Uninitialized.
    {
        let data = unsafe { destination_stake_account_info.borrow_data_unchecked() };
        match StakeStateV2::deserialize(&data) {
            Ok(StakeStateV2::Uninitialized) => { msg!("Split: dest Uninitialized OK"); }
            Ok(_) => { msg!("Split: dest not Uninitialized"); return Err(ProgramError::InvalidAccountData); }
            Err(_) => { msg!("Split: dest deserialize error"); return Err(ProgramError::InvalidAccountData); }
        }
    }

    let source_lamport_balance = source_stake_account_info.lamports();
    let destination_lamport_balance = destination_stake_account_info.lamports();

    if split_lamports > source_lamport_balance {
        return Err(ProgramError::InsufficientFunds);
    }

    match get_stake_state(source_stake_account_info)? {
        StakeStateV2::Stake(source_meta, mut source_stake, stake_flags) => {
            msg!("Split: source=Stake");
            source_meta
                .authorized
                .check(&arr_of_signers, StakeAuthorize::Staker)
                .map_err(to_program_error)?;

            let minimum_delegation = get_minimum_delegation();

            let status = source_stake.delegation.stake_activating_and_deactivating(
                clock.epoch.to_le_bytes(),
                stake_history,
                PERPETUAL_NEW_WARMUP_COOLDOWN_RATE_EPOCH,
            );

            let is_active = bytes_to_u64(status.effective) > 0;

            // NOTE this function also internally summons Rent via syscall
            let validated_split_info = validate_split_amount(
                source_lamport_balance,
                destination_lamport_balance,
                split_lamports,
                &source_meta,
                destination_data_len,
                minimum_delegation,
                is_active,
            )?;

            // split the stake, subtract rent_exempt_balance unless
            // the destination account already has those lamports
            // in place.
            // this means that the new stake account will have a stake equivalent to
            // lamports minus rent_exempt_reserve if it starts out with a zero balance
            let (remaining_stake_delta, split_stake_amount) =
                if validated_split_info.source_remaining_balance == 0 {
                    // If split amount equals the full source stake (as implied by 0
                    // source_remaining_balance), the new split stake must equal the same
                    // amount, regardless of any current lamport balance in the split account.
                    // Since split accounts retain the state of their source account, this
                    // prevents any magic activation of stake by prefunding the split account.
                    //
                    // The new split stake also needs to ignore any positive delta between the
                    // original rent_exempt_reserve and the split_rent_exempt_reserve, in order
                    // to prevent magic activation of stake by splitting between accounts of
                    // different sizes.
                    let remaining_stake_delta = split_lamports
                        .saturating_sub(bytes_to_u64(source_meta.rent_exempt_reserve));
                    (remaining_stake_delta, remaining_stake_delta)
                } else {
                    // Otherwise, the new split stake should reflect the entire split
                    // requested, less any lamports needed to cover the
                    // split_rent_exempt_reserve.
                    if bytes_to_u64(source_stake.delegation.stake).saturating_sub(split_lamports)
                        < minimum_delegation
                    {
                        return Err(to_program_error(StakeError::InsufficientDelegation.into()));
                    }

                    (
                        split_lamports,
                        split_lamports.saturating_sub(
                            validated_split_info
                                .destination_rent_exempt_reserve
                                .saturating_sub(destination_lamport_balance),
                        ),
                    )
                };

            if split_stake_amount < minimum_delegation {
                return Err(to_program_error(StakeError::InsufficientDelegation.into()));
            }

            let destination_stake = source_stake
                .split(remaining_stake_delta, split_stake_amount)
                .map_err(to_program_error)?;

            let mut destination_meta = source_meta;
            destination_meta.rent_exempt_reserve = validated_split_info
                .destination_rent_exempt_reserve
                .to_le_bytes();

            set_stake_state(
                source_stake_account_info,
                &StakeStateV2::Stake(source_meta, source_stake, stake_flags),
            )?;

            set_stake_state(
                destination_stake_account_info,
                &StakeStateV2::Stake(destination_meta, destination_stake, stake_flags),
            )?;
        }
        StakeStateV2::Initialized(source_meta) => {
            msg!("Split: source=Initialized");
            source_meta
                .authorized
                .check(&arr_of_signers, StakeAuthorize::Staker)
                .map_err(to_program_error)?;

            // NOTE this function also internally summons Rent via syscall
            let validated_split_info = validate_split_amount(
                source_lamport_balance,
                destination_lamport_balance,
                split_lamports,
                &source_meta,
                destination_data_len,
                0,     // additional_required_lamports
                false, // is_active
            )?;

            let mut destination_meta = source_meta;
            destination_meta.rent_exempt_reserve = validated_split_info
                .destination_rent_exempt_reserve
                .to_le_bytes();

            set_stake_state(
                destination_stake_account_info,
                &StakeStateV2::Initialized(destination_meta),
            )?;
        }
        StakeStateV2::Uninitialized => {
            msg!("Split: source=Uninitialized");
            if !source_stake_account_info.is_signer() {
                return Err(ProgramError::MissingRequiredSignature);
            }
        }
        _ => { msg!("Split: source invalid state"); return Err(ProgramError::InvalidAccountData) },
    }

    // Deinitialize state upon zero balance
    if split_lamports == source_lamport_balance {
        set_stake_state(source_stake_account_info, &StakeStateV2::Uninitialized)?;
    }

    msg!("Split: relocating lamports");
    relocate_lamports(
        source_stake_account_info,
        destination_stake_account_info,
        split_lamports,
    )?;

    msg!("Split: done");
    Ok(())
}
