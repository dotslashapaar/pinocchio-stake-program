
use core::mem::MaybeUninit;
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
    error::StakeError,
    id,
    state::stake_state::StakeStateV2,
    tools::{acceptable_reference_epoch_credits, eligible_for_deactivate_delinquent},
};

// need this for vote stuff
use solana_vote_program::{self, state::VoteState};

// main function that processes the deactivate delinquent instruction
pub fn process_deactivate_delinquent(accounts: &[AccountInfo]) -> ProgramResult {
    msg!("Instruction: DeactivateDelinquent");

    let account_info_iter = &mut accounts.iter();

    // get the 3 accounts we need
    let stake_account_info          = next_account_info(account_info_iter)?;
    let delinquent_vote_account_info = next_account_info(account_info_iter)?;
    let reference_vote_account_info  = next_account_info(account_info_iter)?;

    let clock = Clock::get()?;

    // get vote states from the accounts
    let delinquent_vote_state = get_vote_state(delinquent_vote_account_info)?;
    let reference_vote_state  = get_vote_state(reference_vote_account_info)?;

    // check if reference account has voted recently
    if !acceptable_reference_epoch_credits(&reference_vote_state.epoch_credits, clock.epoch) {
        return Err(StakeError::InsufficientReferenceVotes.into());
    }

    // get stake account and check stuff
    if let StakeStateV2::Stake(meta, mut stake, stake_flags) = get_stake_state(stake_account_info)? {
        if stake.delegation.voter_pubkey != *delinquent_vote_account_info.key {
            return Err(StakeError::VoteAddressMismatch.into());
        }

        // check if the vote account hasn't voted for a while
        if eligible_for_deactivate_delinquent(&delinquent_vote_state.epoch_credits, clock.epoch) {
            stake.deactivate(clock.epoch)?;
            set_stake_state(stake_account_info,
                            &StakeStateV2::Stake(meta, stake, stake_flags))?;
        } else {
            return Err(StakeError::MinimumDelinquentEpochsForDeactivationNotMet.into());
        }
    } else {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(())
}

// helper functions below

// gets vote state from an account
fn get_vote_state(vote_ai: &AccountInfo) -> Result<Box<VoteState>, ProgramError> {
    if *vote_ai.owner != solana_vote_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    // this is how the original does it with MaybeUninit
    let mut vote_state = Box::new(MaybeUninit::<VoteState>::uninit());
    VoteState::deserialize_into_uninit(&vote_ai.try_borrow_data()?, vote_state.as_mut())
        .map_err(|_| ProgramError::InvalidAccountData)?;
    // this is safe because we just initialized it
    Ok(unsafe { Box::from_raw(Box::into_raw(vote_state) as *mut VoteState) })
}

// gets stake state from account
fn get_stake_state(stake_ai: &AccountInfo) -> Result<StakeStateV2, ProgramError> {
    if *stake_ai.owner != id() {
        return Err(ProgramError::InvalidAccountOwner);
    }
    stake_ai
        .deserialize_data()               // pinocchio has this helper
        .map_err(|_| ProgramError::InvalidAccountData)
}

// saves stake state back to account
fn set_stake_state(stake_ai: &AccountInfo, new_state: &StakeStateV2) -> ProgramResult {
    let size = bincode::serialized_size(new_state)
        .map_err(|_| ProgramError::InvalidAccountData)?;
    if size > stake_ai.data_len() as u64 {
        return Err(ProgramError::AccountDataTooSmall);
    }
    bincode::serialize_into(&mut stake_ai.try_borrow_mut_data()?[..], new_state)
        .map_err(|_| ProgramError::InvalidAccountData)
}