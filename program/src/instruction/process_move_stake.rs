
use pinocchio::{account_info::AccountInfo, program_error::ProgramError, ProgramResult};

use crate::error::{to_program_error, StakeError};
use crate::helpers::{
    bytes_to_u64,
    get_minimum_delegation,
    move_stake_or_lamports_shared_checks,
    next_account_info,
    relocate_lamports, // use shared helper, not a local copy
    set_stake_state,
};
use crate::helpers::merge::merge_delegation_stake_and_credits_observed; // adjust path if you re-export at crate::helpers::*
use crate::state::{MergeKind, StakeFlags, StakeStateV2};

pub fn process_move_stake(accounts: &[AccountInfo], lamports: u64) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    // native asserts: 3 accounts
    let source_stake_account_info = next_account_info(account_info_iter)?;
    let destination_stake_account_info = next_account_info(account_info_iter)?;
    let stake_authority_info = next_account_info(account_info_iter)?;

    let (source_merge_kind, destination_merge_kind) = move_stake_or_lamports_shared_checks(
        source_stake_account_info,
        lamports,
        destination_stake_account_info,
        stake_authority_info,
    )?;

    // ensure source and destination are the right size for the current version of StakeState
    if source_stake_account_info.data_len() != StakeStateV2::size_of()
        || destination_stake_account_info.data_len() != StakeStateV2::size_of()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    // source must be fully active
    let MergeKind::FullyActive(source_meta, mut source_stake) = source_merge_kind else {
        return Err(ProgramError::InvalidAccountData);
    };

    let minimum_delegation = get_minimum_delegation();
    let source_effective_stake = source_stake.delegation.stake;

    // cannot move more stake than the source has (even if it has plenty of lamports)
    let source_final_stake = bytes_to_u64(source_effective_stake)
        .checked_sub(lamports)
        .ok_or(ProgramError::InvalidArgument)?;

    // unless moving all stake, the source must remain at/above the minimum delegation
    if source_final_stake != 0 && source_final_stake < minimum_delegation {
        return Err(ProgramError::InvalidArgument);
    }

    // destination must be fully active or fully inactive
    let destination_meta = match destination_merge_kind {
        MergeKind::FullyActive(destination_meta, mut destination_stake) => {
            // active destination must share the same vote account
            if source_stake.delegation.voter_pubkey != destination_stake.delegation.voter_pubkey {
                return Err(to_program_error(StakeError::VoteAddressMismatch));
            }

            let destination_effective_stake = destination_stake.delegation.stake;
            let destination_final_stake = bytes_to_u64(destination_effective_stake)
                .checked_add(lamports)
                .ok_or(ProgramError::ArithmeticOverflow)?;

            // ensure destination also meets the minimum (relevant if minimum is raised)
            if destination_final_stake < minimum_delegation {
                return Err(ProgramError::InvalidArgument);
            }

            // move stake weight and recompute credits_observed (weighted)
            merge_delegation_stake_and_credits_observed(
                &mut destination_stake,
                lamports,
                bytes_to_u64(source_stake.credits_observed),
            )?;

            // flags cleared for active stake (matches native)
            set_stake_state(
                destination_stake_account_info,
                &StakeStateV2::Stake(destination_meta, destination_stake, StakeFlags::empty()),
            )?;

            destination_meta
        }
        MergeKind::Inactive(destination_meta, _, _) => {
            // inactive destination must receive at least the minimum delegation
            if lamports < minimum_delegation {
                return Err(ProgramError::InvalidArgument);
            }

            // clone source stake shape and set only the moved stake amount
            let mut destination_stake = source_stake;
            destination_stake.delegation.stake = lamports.to_le_bytes();

            set_stake_state(
                destination_stake_account_info,
                &StakeStateV2::Stake(destination_meta, destination_stake, StakeFlags::empty()),
            )?;

            destination_meta
        }
        _ => return Err(ProgramError::InvalidAccountData),
    };

    // write back source: either to Initialized(meta) if emptied, or Stake with reduced stake
    if source_final_stake == 0 {
        set_stake_state(
            source_stake_account_info,
            &StakeStateV2::Initialized(source_meta),
        )?;
    } else {
        source_stake.delegation.stake = source_final_stake.to_le_bytes();
        set_stake_state(
            source_stake_account_info,
            &StakeStateV2::Stake(source_meta, source_stake, StakeFlags::empty()),
        )?;
    }

    // physically move lamports between accounts
    relocate_lamports(
        source_stake_account_info,
        destination_stake_account_info,
        lamports,
    )?;

    // guard against impossible (rent) underflows due to any mismatch in math
    if source_stake_account_info.lamports() < bytes_to_u64(source_meta.rent_exempt_reserve)
        || destination_stake_account_info.lamports()
            < bytes_to_u64(destination_meta.rent_exempt_reserve)
    {
        return Err(ProgramError::InvalidArgument);
    }

    Ok(())
}