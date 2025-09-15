use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    sysvars::{clock::Clock, Sysvar},
};

use crate::{
    helpers::{bytes_to_u64, checked_add, get_stake_state},
    state::{delegation::Stake, MergeKind, StakeAuthorize, StakeHistorySysvar},
};

pub fn stake_weighted_credits_observed(
    stake: &Stake,
    absorbed_lamports: u64,
    absorbed_credits_observed: u64,
) -> Option<u64> {
    if bytes_to_u64(stake.credits_observed) == absorbed_credits_observed {
        Some(bytes_to_u64(stake.credits_observed))
    } else {
        let total_stake =
            u128::from(bytes_to_u64(stake.delegation.stake).checked_add(absorbed_lamports)?);
        let stake_weighted_credits = u128::from(bytes_to_u64(stake.credits_observed))
            .checked_mul(u128::from(bytes_to_u64(stake.delegation.stake)))?;
        let absorbed_weighted_credits =
            u128::from(absorbed_credits_observed).checked_mul(u128::from(absorbed_lamports))?;
        // ceiling: +denominator-1 before division
        let total_weighted_credits = stake_weighted_credits
            .checked_add(absorbed_weighted_credits)?
            .checked_add(total_stake)?
            .checked_sub(1)?;
        u64::try_from(total_weighted_credits.checked_div(total_stake)?).ok()
    }
}

pub fn merge_delegation_stake_and_credits_observed(
    stake: &mut Stake,
    lamports_to_merge: u64,
    source_credits_observed: u64,
) -> Result<(), ProgramError> {
    stake.delegation.stake =
        checked_add(bytes_to_u64(stake.delegation.stake), lamports_to_merge)?.to_le_bytes();
    stake.credits_observed =
        stake_weighted_credits_observed(stake, lamports_to_merge, source_credits_observed)
            .ok_or(ProgramError::ArithmeticOverflow)?
            .to_le_bytes();
    Ok(())
}

pub fn move_stake_or_lamports_shared_checks(
    source_stake_account_info: &AccountInfo,
    lamports: u64,
    destination_stake_account_info: &AccountInfo,
    stake_authority_info: &AccountInfo,
) -> Result<(MergeKind, MergeKind), ProgramError> {
    // Authority must sign
    if !stake_authority_info.is_signer() {
        pinocchio::msg!("shared_checks: missing signer");
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Confirm not the same account
    if *source_stake_account_info.key() == *destination_stake_account_info.key() {
        pinocchio::msg!("shared_checks: same account");
        return Err(ProgramError::InvalidInstructionData);
    }

    // Source and destination must be writable
    if !source_stake_account_info.is_writable() || !destination_stake_account_info.is_writable() {
        pinocchio::msg!("shared_checks: not writable");
        return Err(ProgramError::InvalidInstructionData);
    }

    // Must move something
    if lamports == 0 {
        pinocchio::msg!("shared_checks: zero lamports");
        return Err(ProgramError::InvalidArgument);
    }

    let clock = Clock::get()?;
    let stake_history = StakeHistorySysvar(clock.epoch);

    // Quick sanity logs
    if *source_stake_account_info.owner() != crate::ID {
        pinocchio::msg!("shared_checks: src wrong owner");
    }
    if *destination_stake_account_info.owner() != crate::ID {
        pinocchio::msg!("shared_checks: dst wrong owner");
    }
    if source_stake_account_info.data_len() != crate::state::stake_state_v2::StakeStateV2::size_of() {
        pinocchio::msg!("shared_checks: src size mismatch");
    }
    if destination_stake_account_info.data_len() != crate::state::stake_state_v2::StakeStateV2::size_of() {
        pinocchio::msg!("shared_checks: dst size mismatch");
    }

    // Ensure neither account is transient and both are mergeable
    let source_state = get_stake_state(source_stake_account_info)?;
    let source_merge_kind = match MergeKind::get_if_mergeable(
        &source_state,
        source_stake_account_info.lamports(),
        &clock,
        &stake_history,
    ) {
        Ok(k) => k,
        Err(e) => {
            pinocchio::msg!("shared_checks: source not mergeable");
            return Err(e);
        }
    };

    // Authorized staker check on the source metadata
    let src_meta = source_merge_kind.meta();
    if src_meta.authorized.staker != *stake_authority_info.key() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let destination_state = get_stake_state(destination_stake_account_info)?;
    let destination_merge_kind = match MergeKind::get_if_mergeable(
        &destination_state,
        destination_stake_account_info.lamports(),
        &clock,
        &stake_history,
    ) {
        Ok(k) => k,
        Err(e) => {
            pinocchio::msg!("shared_checks: destination not mergeable");
            return Err(e);
        }
    };

    // Log the classification paths for debugging
    pinocchio::msg!("shared_checks: classified source");
    pinocchio::msg!("shared_checks: classified destination");

    // Ensure metadata is compatible (authorities and lockups)
    if let Err(e) = MergeKind::metas_can_merge(
        source_merge_kind.meta(),
        destination_merge_kind.meta(),
        &clock,
    ) {
        pinocchio::msg!("shared_checks: metas cannot merge");
        return Err(e);
    }

    Ok((source_merge_kind, destination_merge_kind))
}
