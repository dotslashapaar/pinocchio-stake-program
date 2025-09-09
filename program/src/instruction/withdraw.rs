use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    sysvars::clock::Clock,
    ProgramResult,
};

use crate::{
    error::{to_program_error, StakeError},
    helpers::{checked_add, collect_signers_checked, get_stake_state, next_account_info, relocate_lamports, set_stake_state, MAXIMUM_SIGNERS},
    state::{Lockup, StakeAuthorize, StakeHistorySysvar, StakeStateV2},

};

// If these helpers live in the same module as this function, you don't need these imports.
// If they live elsewhere, import them from the right path.
// use crate::processor::{get_stake_state, set_stake_state, relocate_lamports};

pub fn process_withdraw(accounts: &[AccountInfo], withdraw_lamports: u64) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    // native asserts: 5 accounts (2 sysvars)
    let source_stake_account_info = next_account_info(account_info_iter)?;
    let destination_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    let _stake_history_info = next_account_info(account_info_iter)?;
    let withdraw_authority_info = next_account_info(account_info_iter)?;
    // other accounts (optional)
    let option_lockup_authority_info = next_account_info(account_info_iter).ok();

    let clock = &Clock::from_account_info(clock_info)?;
    let stake_history = &StakeHistorySysvar(clock.epoch);

    // Exactly like native: require withdraw authority signer; if custodian account is supplied,
    // it must also be a signer.
    let (signers_set, custodian) =
        collect_signers_checked(Some(withdraw_authority_info), option_lockup_authority_info)?;

    // Authorized::check expects &[[u8; 32]], so convert the set to a compact slice.
    let mut signer_buf = [[0u8; 32]; MAXIMUM_SIGNERS];
    let mut n = 0usize;
    for k in signers_set.iter() {
        if n == MAXIMUM_SIGNERS {
            break;
        }
        signer_buf[n] = *k;
        n += 1;
    }
    let signers_slice: &[[u8; 32]] = &signer_buf[..n];

    // Decide withdrawal constraints based on current stake state
    let (lockup, reserve_u64, is_staked) = match get_stake_state(source_stake_account_info)? {
        StakeStateV2::Stake(meta, stake, _stake_flags) => {
            // Must have withdraw authority
            meta.authorized
                .check(signers_slice, StakeAuthorize::Withdrawer)
                .map_err(to_program_error)?;

            // Convert LE-encoded fields to u64
            let deact_epoch = u64::from_le_bytes(stake.delegation.deactivation_epoch);
            let staked: u64 = if clock.epoch >= deact_epoch {
                // Your Delegation::stake expects LE-encoded epoch + rate
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
            // Must have withdraw authority
            meta.authorized
                .check(signers_slice, StakeAuthorize::Withdrawer)
                .map_err(to_program_error)?;

            let rent_reserve = u64::from_le_bytes(meta.rent_exempt_reserve);
            (meta.lockup, rent_reserve, false)
        }
        StakeStateV2::Uninitialized => {
            // For Uninitialized, the source account itself must sign (native passes it twice)
            let source_key = *source_stake_account_info.key();
            if !signers_set.contains(&source_key) {
                return Err(ProgramError::MissingRequiredSignature);
            }
            (Lockup::default(), 0u64, false)
        }
        _ => return Err(ProgramError::InvalidAccountData),
    };

    // Lockup must be expired or bypassed by a custodian signer
    if lockup.is_in_force(clock, custodian) {
        return Err(to_program_error(StakeError::LockupInForce));
    }

    let stake_account_lamports = source_stake_account_info.lamports();

    if withdraw_lamports == stake_account_lamports {
        // Full withdrawal: can't close if still staked
        if is_staked {
            return Err(ProgramError::InsufficientFunds);
        }
        // Deinitialize state upon zero balance
        set_stake_state(source_stake_account_info, &StakeStateV2::Uninitialized)?;
    } else {
        // Partial withdrawal must not deplete the reserve
        let withdraw_plus_reserve = checked_add(withdraw_lamports, reserve_u64)?;
        if withdraw_plus_reserve > stake_account_lamports {
            return Err(ProgramError::InsufficientFunds);
        }
    }

    // Move lamports after state update (native ordering)
    relocate_lamports(
        source_stake_account_info,
        destination_info,
        withdraw_lamports,
    )?;

    Ok(())
}