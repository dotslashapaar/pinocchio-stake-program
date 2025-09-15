
use pinocchio::{account_info::AccountInfo, program_error::ProgramError, ProgramResult};

use crate::error::{to_program_error, StakeError};
use crate::helpers::{
    bytes_to_u64,
    get_minimum_delegation,
    next_account_info,
    relocate_lamports, // use shared helper, not a local copy
    set_stake_state,
    get_stake_state,
};
use crate::helpers::merge::merge_delegation_stake_and_credits_observed; // adjust path if you re-export at crate::helpers::*
use crate::state::{MergeKind, StakeFlags, StakeStateV2};

pub fn process_move_stake(accounts: &[AccountInfo], lamports: u64) -> ProgramResult {
    pinocchio::msg!("MS: start");
    let account_info_iter = &mut accounts.iter();

    // Expected accounts: 3
    let source_stake_account_info = next_account_info(account_info_iter)?;
    let destination_stake_account_info = next_account_info(account_info_iter)?;
    let stake_authority_info = next_account_info(account_info_iter)?;

    pinocchio::msg!("MS: got accounts");
    // Lightweight local checks instead of full shared merge checks
    if !stake_authority_info.is_signer() {
        pinocchio::msg!("MS: missing signer");
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !source_stake_account_info.is_writable() || !destination_stake_account_info.is_writable() {
        pinocchio::msg!("MS: not writable");
        return Err(ProgramError::InvalidInstructionData);
    }
    if *source_stake_account_info.key() == *destination_stake_account_info.key() {
        pinocchio::msg!("MS: same account");
        return Err(ProgramError::InvalidInstructionData);
    }
    if lamports == 0 {
        pinocchio::msg!("MS: zero lamports");
        return Err(ProgramError::InvalidArgument);
    }

    // Skip strict size equality; rely on (de)serialization bounds checks

    // Load source state and require an active delegation (no deactivation scheduled)
    let (source_meta, mut source_stake) = match get_stake_state(source_stake_account_info) {
        Err(e) => {
            pinocchio::msg!("MoveStake: get_stake_state(src) failed");
            return Err(e);
        }
        Ok(state) => match state {
        StakeStateV2::Stake(meta, stake, _flags) => {
            if bytes_to_u64(stake.delegation.deactivation_epoch) != u64::MAX {
                pinocchio::msg!("MoveStake: source deactivating");
                return Err(ProgramError::InvalidAccountData);
            }
            (meta, stake)
        }
        _ => {
            pinocchio::msg!("MoveStake: source not Stake state");
            return Err(ProgramError::InvalidAccountData);
        }
    },
    };
    pinocchio::msg!("MS: loaded source");

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
    let destination_meta = match get_stake_state(destination_stake_account_info) {
        Err(e) => {
            pinocchio::msg!("MoveStake: get_stake_state(dst) failed");
            return Err(e);
        }
        Ok(state) => match state {
        StakeStateV2::Stake(destination_meta, mut destination_stake, _f) => {
            pinocchio::msg!("MS: dst active");
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

            set_stake_state(
                destination_stake_account_info,
                &StakeStateV2::Stake(destination_meta, destination_stake, StakeFlags::empty()),
            )?;

            destination_meta
        }
        StakeStateV2::Initialized(destination_meta) => {
            pinocchio::msg!("MS: dst inactive");
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
        _other => {
            pinocchio::msg!("MoveStake: destination invalid kind");
            return Err(ProgramError::InvalidAccountData);
        }
    },
    };
    pinocchio::msg!("MS: prepared dst");

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
    pinocchio::msg!("MS: wrote source");

    // physically move lamports between accounts
    relocate_lamports(
        source_stake_account_info,
        destination_stake_account_info,
        lamports,
    )?;
    pinocchio::msg!("MS: moved lamports");

    // guard against impossible (rent) underflows due to any mismatch in math
    if source_stake_account_info.lamports() < bytes_to_u64(source_meta.rent_exempt_reserve)
        || destination_stake_account_info.lamports()
            < bytes_to_u64(destination_meta.rent_exempt_reserve)
    {
        return Err(ProgramError::InvalidArgument);
    }

    Ok(())
}
