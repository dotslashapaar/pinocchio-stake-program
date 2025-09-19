
extern crate alloc;

use pinocchio::{account_info::AccountInfo, program_error::ProgramError, ProgramResult, sysvars::Sysvar};
use crate::helpers::{next_account_info, relocate_lamports};
use crate::helpers::merge::move_stake_or_lamports_shared_checks;
use crate::state::merge_kind::MergeKind;

/// Move withdrawable lamports from one stake account to another.
///
/// Accounts (exactly 3):
/// 0. `[writable]` Source stake account (owned by this program)
/// 1. `[writable]` Destination stake account (owned by this program)
/// 2. `[signer]`   Staker authority (must be the *staker* of the source)
pub fn process_move_lamports(accounts: &[AccountInfo], lamports: u64) -> ProgramResult {
    // Parse accounts
    let iter = &mut accounts.iter();
    let source_stake_ai      = next_account_info(iter)?;
    let destination_stake_ai = next_account_info(iter)?;
    let staker_authority_ai  = next_account_info(iter)?;

    // Pre-check: explicitly reject deactivating accounts (destination or source)
    let clock = pinocchio::sysvars::clock::Clock::get()?;
    // Ensure both are valid stake states and not transiently deactivating
    for (idx, ai) in [source_stake_ai, destination_stake_ai].iter().enumerate() {
        match crate::helpers::get_stake_state(ai)? {
            // Stake: check deactivation window
            crate::state::stake_state_v2::StakeStateV2::Stake(_, stake, _) => {
                let deact = crate::helpers::bytes_to_u64(stake.delegation.deactivation_epoch);
                if deact != u64::MAX && clock.epoch <= deact {
                    return Err(crate::error::to_program_error(
                        crate::error::StakeError::MergeMismatch,
                    ));
                }
            }
            // Initialized: permitted (no deactivation to check)
            crate::state::stake_state_v2::StakeStateV2::Initialized(_) => {
            }
            // Uninitialized or other: invalid
            _ => {
                return Err(ProgramError::InvalidAccountData);
            }
        }
    }

    // Shared checks (signer present, accounts distinct and writable, nonzero amount,
    // classification via MergeKind, and metadata compatibility)
    let (source_kind, dest_kind) = move_stake_or_lamports_shared_checks(
        source_stake_ai,
        lamports,
        destination_stake_ai,
        staker_authority_ai,
        true,  // enforce meta compatibility (authorities, lockups)
        false, // do not require mergeable classification
    )?;

    // Extra guard for lamports: require identical authorities between source and destination
    let src_auth = &source_kind.meta().authorized;
    let dst_auth = &dest_kind.meta().authorized;
    if src_auth != dst_auth {
        return Err(crate::error::to_program_error(crate::error::StakeError::MergeMismatch));
    }

    // (post-check logging removed; pre-check above handles transient)

    // Additional authority check: the staker must authorize this movement
    if source_kind.meta().authorized.staker != *staker_authority_ai.key() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Compute how many lamports are available to move from source:
    //  - FullyActive: lamports - delegated - rent_exempt_reserve
    //  - Inactive:   lamports - rent_exempt_reserve
    //  - Activating/deactivating: not allowed
    let source_free_lamports = match source_kind {
        MergeKind::FullyActive(ref meta, ref stake) => {
            let rent_reserve = u64::from_le_bytes(meta.rent_exempt_reserve);
            let delegated    = u64::from_le_bytes(stake.delegation.stake);

            source_stake_ai
                .lamports()
                .saturating_sub(delegated)
                .saturating_sub(rent_reserve)
        }
        MergeKind::Inactive(ref meta, source_lamports, _flags) => {
            let rent_reserve = u64::from_le_bytes(meta.rent_exempt_reserve);
            source_lamports.saturating_sub(rent_reserve)
        }
        _ => {
            // Partially activating/deactivating is not allowed for MoveLamports
            return Err(crate::error::to_program_error(crate::error::StakeError::MergeMismatch));
        }
    };

    // Amount must be within the available budget
    if lamports > source_free_lamports {
        return Err(ProgramError::InvalidArgument);
    }

    // Move lamports
    relocate_lamports(source_stake_ai, destination_stake_ai, lamports)?;

    // Post-condition: both accounts must remain at/above their rent reserves
    let src_meta = source_kind.meta();
    let dst_meta = dest_kind.meta();
    if source_stake_ai.lamports() < u64::from_le_bytes(src_meta.rent_exempt_reserve)
        || destination_stake_ai.lamports() < u64::from_le_bytes(dst_meta.rent_exempt_reserve)
    {
        return Err(ProgramError::InvalidArgument);
    }

    Ok(())
}
