// This file handles moving lamports between stake accounts

//  need hashbrown for HashSet since we're no_std
use hashbrown::HashSet; 

use pinocchio::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvar::Sysvar,
};

use crate::{
    helpers::{to_program_error, StakeHistorySysvar},
    id,
    state::{
        merge_kind::MergeKind,
        stake_authorize::StakeAuthorize,
        stake_state::StakeStateV2,
    },
};

// main function for processing move lamports
pub fn process_move_lamports(accounts: &[AccountInfo], lamports: u64) -> ProgramResult {
    msg!("Instruction: MoveLamports");

    let account_info_iter = &mut accounts.iter();

    // need 3 accounts for this
    let source_stake_ai      = next_account_info(account_info_iter)?;
    let destination_stake_ai = next_account_info(account_info_iter)?;
    let stake_authority_ai   = next_account_info(account_info_iter)?;

    // check if everything is valid (shares code with MoveStake)
    let (source_merge_kind, _) = move_stake_or_lamports_shared_checks(
        source_stake_ai,
        lamports,
        destination_stake_ai,
        stake_authority_ai,
    )?;

    // figure out how many lamports we can actually move
    let source_free_lamports = match source_merge_kind {
        MergeKind::FullyActive(source_meta, source_stake) => source_stake_ai
            .lamports()
            .saturating_sub(source_stake.delegation.stake)
            .saturating_sub(source_meta.rent_exempt_reserve),
        MergeKind::Inactive(source_meta, source_lamports, _inactive_stake) => {
            source_lamports.saturating_sub(source_meta.rent_exempt_reserve)
        }
        _ => return Err(ProgramError::InvalidAccountData),
    };

    if lamports > source_free_lamports {
        return Err(ProgramError::InvalidArgument);
    }

    relocate_lamports(source_stake_ai, destination_stake_ai, lamports)
}

// this function does checks that both MoveStake and MoveLamports need
fn move_stake_or_lamports_shared_checks(
    source_stake_ai:      &AccountInfo,
    lamports:             u64,
    destination_stake_ai: &AccountInfo,
    stake_authority_ai:   &AccountInfo,
) -> Result<(MergeKind, MergeKind), ProgramError> {
    // check authority signed
    let (signers, _) = collect_signers_checked(Some(stake_authority_ai), None)?;

    // make sure not same account
    if source_stake_ai.key == destination_stake_ai.key {
        return Err(ProgramError::InvalidInstructionData);
    }

    // both need to be writable
    if !source_stake_ai.is_writable || !destination_stake_ai.is_writable {
        return Err(ProgramError::InvalidInstructionData);
    }

    // can't move 0 lamports
    if lamports == 0 {
        return Err(ProgramError::InvalidArgument);
    }

    // do the merge kind checks
    let clock         = Clock::get()?;
    let stake_history = StakeHistorySysvar(clock.epoch);

    let source_merge_kind = MergeKind::get_if_mergeable(
        &get_stake_state(source_stake_ai)?,
        source_stake_ai.lamports(),
        &clock,
        &stake_history,
    )?;

    // check staker authority
    source_merge_kind
        .meta()
        .authorized
        .check(&signers, StakeAuthorize::Staker)
        .map_err(to_program_error)?;

    let destination_merge_kind = MergeKind::get_if_mergeable(
        &get_stake_state(destination_stake_ai)?,
        destination_stake_ai.lamports(),
        &clock,
        &stake_history,
    )?;

    // check if accounts can merge
    MergeKind::metas_can_merge(
        source_merge_kind.meta(),
        destination_merge_kind.meta(),
        &clock,
    )?;

    Ok((source_merge_kind, destination_merge_kind))
}

// helper functions

// collects signers from the accounts
fn collect_signers_checked<'a>(
    authority_info:  Option<&'a AccountInfo>,
    custodian_info:  Option<&'a AccountInfo>,
) -> Result<(HashSet<Pubkey>, Option<&'a Pubkey>), ProgramError> {
    let mut signers = HashSet::new();

    if let Some(ai) = authority_info {
        if ai.is_signer {
            signers.insert(*ai.key);
        } else {
            return Err(ProgramError::MissingRequiredSignature);
        }
    }

    let custodian = if let Some(ci) = custodian_info {
        if ci.is_signer {
            signers.insert(*ci.key);
            Some(ci.key)
        } else {
            return Err(ProgramError::MissingRequiredSignature);
        }
    } else {
        None
    };

    Ok((signers, custodian))
}

// gets the stake state from an account
fn get_stake_state(stake_ai: &AccountInfo) -> Result<StakeStateV2, ProgramError> {
    if *stake_ai.owner != id() {
        return Err(ProgramError::InvalidAccountOwner);
    }
    stake_ai.deserialize_data().map_err(|_| ProgramError::InvalidAccountData)
}

// moves lamports from one account to another (not called "move" because there's a MoveLamports instruction)
fn relocate_lamports(
    source_ai:      &AccountInfo,
    destination_ai: &AccountInfo,
    lamports:       u64,
) -> ProgramResult {
    {
        let mut from_lamports = source_ai.try_borrow_mut_lamports()?;
        **from_lamports = from_lamports
            .checked_sub(lamports)
            .ok_or(ProgramError::InsufficientFunds)?;
    }
    {
        let mut to_lamports = destination_ai.try_borrow_mut_lamports()?;
        **to_lamports = to_lamports
            .checked_add(lamports)
            .ok_or(ProgramError::ArithmeticOverflow)?;
    }
    Ok(())
}