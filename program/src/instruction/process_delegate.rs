use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::{clock::Clock, Sysvar},
    ProgramResult,
};

use crate::error::to_program_error;
use crate::helpers::utils::{
    get_stake_state, get_vote_state, new_stake, redelegate_stake, set_stake_state,
    validate_delegated_amount, ValidatedDelegatedInfo,
};
use crate::helpers::*;
use crate::state::stake_history::StakeHistorySysvar;
use crate::state::{StakeAuthorize, StakeFlags, StakeStateV2};

pub fn process_delegate(accounts: &[AccountInfo]) -> ProgramResult {
    // Collect signers (caller-provided helper)
    let mut signers_array = [Pubkey::default(); MAXIMUM_SIGNERS];
    let signers_count = collect_signers(accounts, &mut signers_array)?;
    let signers = &signers_array[..signers_count];

    // expected: [stake, vote, clock, stake_history, stake_config]
    let mut it = accounts.iter();
    let stake_account_info = it.next().ok_or(ProgramError::NotEnoughAccountKeys)?;
    let vote_account_info  = it.next().ok_or(ProgramError::NotEnoughAccountKeys)?;
    let _clock_info        = it.next().ok_or(ProgramError::NotEnoughAccountKeys)?;
    let _stake_history_info= it.next().ok_or(ProgramError::NotEnoughAccountKeys)?;
    let _stake_config_info = it.next().ok_or(ProgramError::NotEnoughAccountKeys)?;

    // Use sysvar getter (Pinocchio-friendly)
    let clock = Clock::get()?;
    let stake_history = StakeHistorySysvar(clock.epoch);

    // Read vote state via helper (should deserialize from account bytes)
    let vote_state = get_vote_state(vote_account_info)?;

    match get_stake_state(stake_account_info)? {
        StakeStateV2::Initialized(meta) => {
            // staker must sign
            meta.authorized
                .check(signers, StakeAuthorize::Staker)?;

            // how much lamports are available to delegate
            let ValidatedDelegatedInfo { stake_amount } =
                validate_delegated_amount(stake_account_info, &meta)?;

            // build stake object and store state
            let stake = new_stake(
                stake_amount,
                vote_account_info.key(),
                &vote_state,
                clock.epoch,
            );

            set_stake_state(
                stake_account_info,
                &StakeStateV2::Stake(meta, stake, StakeFlags::empty()),
            )
        }
        StakeStateV2::Stake(meta, mut stake, flags) => {
            // staker must sign
            meta.authorized
                .check(signers, StakeAuthorize::Staker)?;

            let ValidatedDelegatedInfo { stake_amount } =
                validate_delegated_amount(stake_account_info, &meta)?;

            // If switching to a different vote account, only allow when inactive
           let is_active = stake.delegation.deactivation_epoch == u64::MAX
    || stake.delegation.deactivation_epoch > clock.epoch;
if is_active {
    return Err(ProgramError::InvalidArgument);
}

            redelegate_stake(
                &mut stake,
                stake_amount,
                vote_account_info.key(),
                &vote_state,
                clock.epoch,
                &stake_history,
            )?;

            set_stake_state(stake_account_info, &StakeStateV2::Stake(meta, stake, flags))
        }
        _ => Err(ProgramError::InvalidAccountData),
    }?;

    Ok(())
}
