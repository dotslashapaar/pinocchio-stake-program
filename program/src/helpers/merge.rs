use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    sysvars::{clock::Clock, Sysvar},
};

use crate::{
    helpers::{bytes_to_u64, checked_add, get_stake_state},
    state::{delegation::Stake, MergeKind, StakeHistorySysvar},
};
use crate::error::{to_program_error, StakeError};

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

fn classify_loose(
    state: &crate::state::stake_state_v2::StakeStateV2,
    stake_lamports: u64,
    clock: &Clock,
) -> Result<MergeKind, ProgramError> {
    use crate::state::stake_state_v2::StakeStateV2 as SS;
    match state {
        SS::Stake(meta, stake, flags) => {
            let act = bytes_to_u64(stake.delegation.activation_epoch);
            let deact = bytes_to_u64(stake.delegation.deactivation_epoch);
            // Transient deactivating should have been filtered earlier by caller
            if deact != u64::MAX && clock.epoch > deact {
                // Fully deactivated -> treat as Inactive
                Ok(MergeKind::Inactive(*meta, stake_lamports, *flags))
            } else if clock.epoch >= act && deact == u64::MAX {
                Ok(MergeKind::FullyActive(*meta, *stake))
            } else {
                Ok(MergeKind::ActivationEpoch(*meta, *stake, *flags))
            }
        }
        SS::Initialized(meta) => Ok(MergeKind::Inactive(*meta, stake_lamports, crate::state::stake_flag::StakeFlags::empty())),
        _ => Err(ProgramError::InvalidAccountData),
    }
}

pub fn move_stake_or_lamports_shared_checks(
    source_stake_account_info: &AccountInfo,
    lamports: u64,
    destination_stake_account_info: &AccountInfo,
    stake_authority_info: &AccountInfo,
    require_meta_compat: bool,
    require_mergeable: bool,
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

    // Quick discriminant-based invalidation for Uninitialized
    {
        let data = unsafe { source_stake_account_info.borrow_data_unchecked() };
        if !data.is_empty() && data[0] == 0 {
            return Err(ProgramError::InvalidAccountData);
        }
    }
    {
        let data = unsafe { destination_stake_account_info.borrow_data_unchecked() };
        if !data.is_empty() && data[0] == 0 {
            return Err(ProgramError::InvalidAccountData);
        }
    }

    // Ensure neither account is transient and both are mergeable
    let source_state = get_stake_state(source_stake_account_info)?;
    // Uninitialized as source is invalid for both move_lamports and move_stake
    if let crate::state::stake_state_v2::StakeStateV2::Uninitialized = &source_state {
        return Err(ProgramError::InvalidAccountData);
    }
    match &source_state {
        crate::state::stake_state_v2::StakeStateV2::Stake(_, _, _) => pinocchio::msg!("shared_checks: src_state=Stake"),
        crate::state::stake_state_v2::StakeStateV2::Initialized(_) => pinocchio::msg!("shared_checks: src_state=Init"),
        crate::state::stake_state_v2::StakeStateV2::Uninitialized => {
            pinocchio::msg!("shared_checks: src_state=Uninit");
            return Err(ProgramError::InvalidAccountData);
        }
        _ => pinocchio::msg!("shared_checks: src_state=Other"),
    }
    let source_merge_kind = match MergeKind::get_if_mergeable(
        &source_state,
        source_stake_account_info.lamports(),
        &clock,
        &stake_history,
    ) {
        Ok(k) => k,
        Err(e) => {
            // Map Uninitialized to InvalidAccountData explicitly
            if matches!(source_state, crate::state::stake_state_v2::StakeStateV2::Uninitialized) {
                return Err(ProgramError::InvalidAccountData);
            }
            if require_mergeable {
                pinocchio::msg!("shared_checks: source not mergeable");
                return Err(e);
            } else {
                classify_loose(&source_state, source_stake_account_info.lamports(), &clock)?
            }
        }
    };
    // Transient guard: reject deactivating sources explicitly (matches native)
    if let crate::state::stake_state_v2::StakeStateV2::Stake(_, stake, _) = &source_state {
        let clock = Clock::get()?;
        let deact = bytes_to_u64(stake.delegation.deactivation_epoch);
        if deact != u64::MAX && clock.epoch <= deact {
            pinocchio::msg!("shared_checks: source deactivating");
            return Err(to_program_error(StakeError::MergeMismatch));
        }
    }

    // Debug classification
    match &source_merge_kind {
        MergeKind::FullyActive(_, _) => pinocchio::msg!("shared_checks: src=FA"),
        MergeKind::Inactive(_, _, _) => pinocchio::msg!("shared_checks: src=IN"),
        MergeKind::ActivationEpoch(_, _, _) => pinocchio::msg!("shared_checks: src=AE"),
    }

    // Authorized staker check on the source metadata
    let src_meta = source_merge_kind.meta();
    if src_meta.authorized.staker != *stake_authority_info.key() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Peek destination discriminant
    {
        let data = unsafe { destination_stake_account_info.borrow_data_unchecked() };
        if !data.is_empty() {
            if data[0] == 2 { pinocchio::msg!("shared_checks: dst_disc=Stake"); }
            else if data[0] == 1 { pinocchio::msg!("shared_checks: dst_disc=Init"); }
            else if data[0] == 0 { pinocchio::msg!("shared_checks: dst_disc=Uninit"); }
            else { pinocchio::msg!("shared_checks: dst_disc=Other"); }
        }
    }
    let destination_state = get_stake_state(destination_stake_account_info)?;
    if let crate::state::stake_state_v2::StakeStateV2::Uninitialized = &destination_state {
        return Err(ProgramError::InvalidAccountData);
    }
    // Transient guard: reject deactivating destinations explicitly (matches native)
    if let crate::state::stake_state_v2::StakeStateV2::Stake(_, stake, _) = &destination_state {
        let clock = Clock::get()?;
        let deact = bytes_to_u64(stake.delegation.deactivation_epoch);
        if deact != u64::MAX && clock.epoch <= deact {
            pinocchio::msg!("shared_checks: destination deactivating");
            return Err(to_program_error(StakeError::MergeMismatch));
        }
    }
    match &destination_state {
        crate::state::stake_state_v2::StakeStateV2::Stake(_, _, _) => pinocchio::msg!("shared_checks: dst_state=Stake"),
        crate::state::stake_state_v2::StakeStateV2::Initialized(_) => pinocchio::msg!("shared_checks: dst_state=Init"),
        crate::state::stake_state_v2::StakeStateV2::Uninitialized => {
            pinocchio::msg!("shared_checks: dst_state=Uninit");
            return Err(ProgramError::InvalidAccountData);
        }
        _ => pinocchio::msg!("shared_checks: dst_state=Other"),
    }
    let destination_merge_kind = match MergeKind::get_if_mergeable(
        &destination_state,
        destination_stake_account_info.lamports(),
        &clock,
        &stake_history,
    ) {
        Ok(k) => k,
        Err(e) => {
            // Map Uninitialized to InvalidAccountData explicitly
            if matches!(destination_state, crate::state::stake_state_v2::StakeStateV2::Uninitialized) {
                return Err(ProgramError::InvalidAccountData);
            }
            if require_mergeable {
                pinocchio::msg!("shared_checks: destination not mergeable");
                return Err(e);
            } else {
                classify_loose(&destination_state, destination_stake_account_info.lamports(), &clock)?
            }
        }
    };
    match &destination_merge_kind {
        MergeKind::FullyActive(_, _) => pinocchio::msg!("shared_checks: dst=FA"),
        MergeKind::Inactive(_, _, _) => pinocchio::msg!("shared_checks: dst=IN"),
        MergeKind::ActivationEpoch(_, _, _) => pinocchio::msg!("shared_checks: dst=AE"),
    }

    pinocchio::msg!("shared_checks: classified source");
    pinocchio::msg!("shared_checks: classified destination");

    // Ensure metadata is compatible (authorities and lockups) when required
    if require_meta_compat {
        if let Err(e) = MergeKind::metas_can_merge(
            source_merge_kind.meta(),
            destination_merge_kind.meta(),
            &clock,
        ) {
            pinocchio::msg!("shared_checks: metas cannot merge");
            return Err(e);
        }
    }

    Ok((source_merge_kind, destination_merge_kind))
}
