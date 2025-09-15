use pinocchio::{
    account_info::AccountInfo,
    msg,
    program_error::ProgramError,
    sysvars::clock::Clock,
    ProgramResult,
};

use crate::{
    error::{to_program_error, StakeError},
    helpers::{checked_add, get_stake_state, next_account_info, relocate_lamports, set_stake_state},
    state::{Lockup, StakeAuthorize, StakeHistorySysvar, StakeStateV2},

};
use pinocchio::pubkey::Pubkey;

// If these helpers live in the same module as this function, you don't need these imports.
// If they live elsewhere, import them from the right path.
// use crate::processor::{get_stake_state, set_stake_state, relocate_lamports};

pub fn process_withdraw(accounts: &[AccountInfo], withdraw_lamports: u64) -> ProgramResult {
    msg!("Withdraw: enter");
    let account_info_iter = &mut accounts.iter();

    // Expected accounts: 5 (including 2 sysvars)
    let source_stake_account_info = next_account_info(account_info_iter)?;
    let destination_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    let _stake_history_info = next_account_info(account_info_iter)?;
    let withdraw_authority_info = next_account_info(account_info_iter)?;
    // other accounts (optional)
    let option_lockup_authority_info = next_account_info(account_info_iter).ok();

    // Fast path: Uninitialized source with source signer — no sysvars needed
    match get_stake_state(source_stake_account_info) {
        Ok(StakeStateV2::Uninitialized) => {
            msg!("Withdraw: source=Uninitialized fast path");
            if !source_stake_account_info.is_signer() {
                return Err(ProgramError::MissingRequiredSignature);
            }
            relocate_lamports(
                source_stake_account_info,
                destination_info,
                withdraw_lamports,
            )?;
            return Ok(());
        }
        _ => {}
    }

    msg!("Withdraw: load clock");
    let clock = &Clock::from_account_info(clock_info)?;
    let stake_history = &StakeHistorySysvar(clock.epoch);

    // Require withdraw authority signer; if custodian account is supplied it must also be a signer
    msg!("Withdraw: gather signers");
    let mut signer_keys: [Pubkey; 2] = [Pubkey::default(); 2];
    let mut n = 0usize;
    if withdraw_authority_info.is_signer() {
        signer_keys[n] = *withdraw_authority_info.key();
        n += 1;
    } else {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let custodian: Option<&Pubkey> = match option_lockup_authority_info {
        Some(ai) => {
            if ai.is_signer() {
                signer_keys[n] = *ai.key();
                n += 1;
                Some(ai.key())
            } else {
                return Err(ProgramError::MissingRequiredSignature);
            }
        }
        None => None,
    };
    let signers_slice: &[Pubkey] = &signer_keys[..n];

    // Decide withdrawal constraints based on current stake state
    msg!("Withdraw: read state");
    let (lockup, reserve_u64, is_staked) = match get_stake_state(source_stake_account_info)? {
        StakeStateV2::Stake(meta, stake, _stake_flags) => {
            msg!("Withdraw: state=Stake");
            // Must have withdraw authority
            meta.authorized
                .check(signers_slice, StakeAuthorize::Withdrawer)
                .map_err(to_program_error)?;

            // Convert little-endian fields to u64
            let deact_epoch = u64::from_le_bytes(stake.delegation.deactivation_epoch);
            let staked: u64 = if clock.epoch >= deact_epoch {
                // Delegation::stake expects little-endian epoch + rate
                stake.delegation.stake(
                    clock.epoch.to_le_bytes(),
                    stake_history,
                    crate::helpers::PERPETUAL_NEW_WARMUP_COOLDOWN_RATE_EPOCH,
                )
            } else {
                u64::from_le_bytes(stake.delegation.stake)
            };

            let rent_reserve = u64::from_le_bytes(meta.rent_exempt_reserve);
            let staked_plus_reserve = checked_add(staked, rent_reserve)?;
            (meta.lockup, staked_plus_reserve, staked != 0)
        }
        StakeStateV2::Initialized(meta) => {
            msg!("Withdraw: state=Initialized");
            // Must have withdraw authority
            meta.authorized
                .check(signers_slice, StakeAuthorize::Withdrawer)
                .map_err(to_program_error)?;

            let rent_reserve = u64::from_le_bytes(meta.rent_exempt_reserve);
            (meta.lockup, rent_reserve, false)
        }
        StakeStateV2::Uninitialized => {
            // For Uninitialized, require the source account to be a signer
            if !source_stake_account_info.is_signer() {
                return Err(ProgramError::MissingRequiredSignature);
            }
            (Lockup::default(), 0u64, false)
        }
        _ => return Err(ProgramError::InvalidAccountData),
    };

    // Lockup must be expired or bypassed by a custodian signer
    msg!("Withdraw: check lockup");
    if lockup.is_in_force(clock, custodian) {
        return Err(to_program_error(StakeError::LockupInForce));
    }

    let stake_account_lamports = source_stake_account_info.lamports();

    if withdraw_lamports == stake_account_lamports {
        msg!("Withdraw: full");
        // Full withdrawal: can't close if still staked
        if is_staked {
            return Err(ProgramError::InsufficientFunds);
        }
        // Deinitialize state upon zero balance
        set_stake_state(source_stake_account_info, &StakeStateV2::Uninitialized)?;
    } else {
        msg!("Withdraw: partial");
        // Partial withdrawal must not deplete the reserve
        let withdraw_plus_reserve = checked_add(withdraw_lamports, reserve_u64)?;
        if withdraw_plus_reserve > stake_account_lamports {
            return Err(ProgramError::InsufficientFunds);
        }
    }

    // Move lamports after state update
    msg!("Withdraw: relocate lamports");
    relocate_lamports(
        source_stake_account_info,
        destination_info,
        withdraw_lamports,
    )?;

    msg!("Withdraw: ok");
    Ok(())
}
