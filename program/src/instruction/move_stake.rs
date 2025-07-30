use pinocchio::{
    account_info::AccountInfo, 
    program_error::ProgramError,
    ProgramResult,
    msg,
};

use crate::helpers::{deserialize_stake_state, merge_delegation_stake_and_credits_observed, move_stake_or_lamports_shared_checks, serialize_stake_state};
use crate::error::StakeError;
use crate::state::{MergeKind, StakeFlags, StakeStateV2};



// Helper functions for stake state operations
pub fn get_stake_state(stake_account_info: &AccountInfo) -> Result<StakeStateV2, ProgramError> {
    if stake_account_info.owner() != &crate::id() {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    // Use our manual deserialization function
    deserialize_stake_state(stake_account_info)
}

pub fn set_stake_state(
    stake_account_info: &AccountInfo,
    new_state: &StakeStateV2,
) -> Result<(), ProgramError> {
    // Verify account ownership
    if stake_account_info.owner() != &crate::id() {
        return Err(ProgramError::InvalidAccountOwner);
    }
    
    // Verify the account is writable
    if !stake_account_info.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }
    
    // Use our manual serialization function
    serialize_stake_state(stake_account_info, new_state)
}

fn relocate_lamports(
    _source_account: &AccountInfo,
    _destination_account: &AccountInfo,
    _lamports: u64,
) -> Result<(), ProgramError> {
    // Placeholder implementation for lamports transfer
    // this would handle the actual lamport transfer
    // between accounts using the appropriate methods
    Ok(())
}

pub fn move_stake(
    accounts: &[AccountInfo],
    lamports: u64,
) -> ProgramResult {
    // direct array indexing with length check
    if accounts.len() < 3 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    let source_stake_account_info = &accounts[0];
    let destination_stake_account_info = &accounts[1];
    let stake_authority_info = &accounts[2];

    let (source_merge_kind, destination_merge_kind) = move_stake_or_lamports_shared_checks(
        source_stake_account_info,
        lamports,
        destination_stake_account_info,
        stake_authority_info,
    )?;

    // Ensure source and destination are the right size for the current version of
    // StakeState - safeguard in case there is a new version of the struct
    if source_stake_account_info.data_len() != StakeStateV2::size_of()
        || destination_stake_account_info.data_len() != StakeStateV2::size_of()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    // Source must be fully active
    let MergeKind::FullyActive(source_meta, mut source_stake) = source_merge_kind else {
        return Err(ProgramError::InvalidAccountData);
    };

    let minimum_delegation = crate::get_minimum_delegation();
    let source_effective_stake = source_stake.delegation.stake;

    // Source cannot move more stake than it has, regardless of how many lamports it has
    let source_final_stake = source_effective_stake
        .checked_sub(lamports)
        .ok_or(ProgramError::InvalidArgument)?;

    // unless all stake is being moved, source must retain at least the minimum delegation
    if source_final_stake != 0 && source_final_stake < minimum_delegation {
        return Err(ProgramError::InvalidArgument);
    }

    // destination must be fully active or fully inactive
    let destination_meta = match destination_merge_kind {
        MergeKind::FullyActive(destination_meta, mut destination_stake) => {
            // If active, destination must be delegated to the same vote account as source
            if source_stake.delegation.voter_pubkey != destination_stake.delegation.voter_pubkey {
                return Err(StakeError::VoteAddressMismatch.into());
            }

            let destination_effective_stake = destination_stake.delegation.stake;
            let destination_final_stake = destination_effective_stake
                .checked_add(lamports)
                .ok_or(ProgramError::ArithmeticOverflow)?;

            // Ensure destination meets minimum delegation
            // Since it is already active, this only really applies if the minimum is raised
            if destination_final_stake < minimum_delegation {
                return Err(ProgramError::InvalidArgument);
            }

            merge_delegation_stake_and_credits_observed(
                &mut destination_stake,
                lamports,
                source_stake.credits_observed,
            )?;

            // StakeFlags::empty() is valid here because the only existing stake flag,
            // MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED, does not apply to active stakes
            set_stake_state(
                destination_stake_account_info,
                &StakeStateV2::Stake(destination_meta.clone(), destination_stake, StakeFlags::empty()),
            )?;

            destination_meta
        }
        MergeKind::Inactive(destination_meta, _, _) => {
            // If destination is inactive, it must be given at least the minimum delegation
            if lamports < minimum_delegation {
                return Err(ProgramError::InvalidArgument);
            }

            let mut destination_stake = source_stake.clone();
            destination_stake.delegation.stake = lamports;

            // StakeFlags::empty() is valid here because the only existing stake flag,
            // MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED, is cleared when a stake is activated
            set_stake_state(
                destination_stake_account_info,
                &StakeStateV2::Stake(destination_meta.clone(), destination_stake, StakeFlags::empty()),
            )?;

            destination_meta
        }
        _ => return Err(ProgramError::InvalidAccountData),
    };

    if source_final_stake == 0 {
        set_stake_state(
            source_stake_account_info,
            &StakeStateV2::Initialized(source_meta.clone()),
        )?;
    } else {
        source_stake.delegation.stake = source_final_stake;

        // StakeFlags::empty() is valid here because the only existing stake flag,
        // MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED, does not apply to active stakes
        set_stake_state(
            source_stake_account_info,
            &StakeStateV2::Stake(source_meta.clone(), source_stake, StakeFlags::empty()),
        )?;
    }

    relocate_lamports(
        source_stake_account_info,
        destination_stake_account_info,
        lamports,
    )?;

    // This should be impossible, but because we do all our math with delegations,
    // best to guard it
    if source_stake_account_info.lamports() < source_meta.rent_exempt_reserve
        || destination_stake_account_info.lamports() < destination_meta.rent_exempt_reserve
    {
        msg!("Delegation calculations violated lamport balance assumptions");
        return Err(ProgramError::InvalidArgument);
    }

    Ok(())
}