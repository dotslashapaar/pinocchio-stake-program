
extern crate alloc;

use pinocchio::{account_info::AccountInfo, program_error::ProgramError, ProgramResult};
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

    // Shared checks (signer present, accounts distinct and writable, nonzero amount,
    // classification via MergeKind, and metadata compatibility)
    let (source_kind, _dest_kind) = move_stake_or_lamports_shared_checks(
        source_stake_ai,
        lamports,
        destination_stake_ai,
        staker_authority_ai,
    )?;

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
            return Err(ProgramError::InvalidAccountData);
        }
    };

    // Amount must be within the available budget
    if lamports > source_free_lamports {
        return Err(ProgramError::InvalidArgument);
    }

    // Move lamports
    relocate_lamports(source_stake_ai, destination_stake_ai, lamports)
}
