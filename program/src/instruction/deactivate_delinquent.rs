
#![allow(clippy::result_large_err)]


use alloc::boxed::Box;
use alloc::vec::Vec;
use crate::vote_state::{VoteState, vote_program_id, parse_epoch_credits};
use pinocchio::{
    account_info::AccountInfo,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::{clock::Clock, Sysvar},
};

use crate::{
    error::StakeError,
    id,
    state::stake_state_v2::StakeStateV2,
};


pub fn process_deactivate_delinquent(accounts: &[AccountInfo]) -> Result<(), ProgramError> {
    msg!("Instruction: DeactivateDelinquent");

    let mut iter = accounts.iter();
    let stake_ai = iter.next().ok_or(ProgramError::NotEnoughAccountKeys)?;
    let delinquent_vote_ai = iter.next().ok_or(ProgramError::NotEnoughAccountKeys)?;
    let reference_vote_ai = iter.next().ok_or(ProgramError::NotEnoughAccountKeys)?;
    
    let clock = Clock::get()?;

    let delinquent_vs = vote_state(delinquent_vote_ai)?;
    let reference_vs = vote_state(reference_vote_ai)?;
//Check Reference Validator is Active
    if !acceptable_reference_epoch_credits(&reference_vs.epoch_credits, clock.epoch) {
        return Err(StakeError::InsufficientReferenceVotes.into());
    }

    match stake_state(stake_ai)? {
        StakeStateV2::Stake(meta, mut stake, flags) => {
           //Verify Delegation Match
            if stake.delegation.voter_pubkey != *delinquent_vote_ai.key() {
                return Err(StakeError::VoteAddressMismatch.into());
            }
            //Check Delinquency and Deactivate
            if eligible_for_deactivate_delinquent(&delinquent_vs.epoch_credits, clock.epoch) {
                stake.delegation.deactivation_epoch = clock.epoch;
                overwrite_stake_state(stake_ai, &StakeStateV2::Stake(meta, stake, flags))
            } else {
                Err(StakeError::MinimumDelinquentEpochsForDeactivationNotMet.into())
            }
        }
        _ => Err(ProgramError::InvalidAccountData),
    }
}

// ========== HELPER FUNCTIONS ==========
//checks the account is owned by the Vote program
fn vote_state(ai: &AccountInfo) -> Result<Box<VoteState>, ProgramError> {
    if ai.owner() != &vote_program_id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    let data = ai.try_borrow_data()?;
    let credits = parse_epoch_credits(&data).ok_or(ProgramError::InvalidAccountData)?;
    Ok(Box::new(VoteState {
        node_pubkey: Pubkey::default(),
        authorized_withdrawer: Pubkey::default(),
        commission: 0,
        votes: [0; 1600],
        root_slot: None,
        epoch_credits: credits,
    }))
}
//Checks the account is owned by our Stake program
fn stake_state(ai: &AccountInfo) -> Result<StakeStateV2, ProgramError> {
    if ai.owner() != &id() {
        return Err(ProgramError::InvalidAccountOwner);
    }
    let data = ai.try_borrow_data()?;
    StakeStateV2::deserialize(&data)
}
//Ensures we own the account and it's marked as writable before modifying.
fn overwrite_stake_state(ai: &AccountInfo, s: &StakeStateV2) -> Result<(), ProgramError> {
    if ai.owner() != &id() || !ai.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }
    let mut data = ai.try_borrow_mut_data()?;
    s.serialize(&mut data)
}
//Returns true if the validator voted within the last epoch (difference ≤ 1)
fn acceptable_reference_epoch_credits(
    epoch_credits: &[(u64, u64, u64)], 
    current_epoch: u64
) -> bool {
    epoch_credits.iter().any(|(epoch, _, _)| {
        current_epoch.saturating_sub(*epoch) <= 1
    })
}
//Returns true if NO votes exist within the last 4 epochs 
fn eligible_for_deactivate_delinquent(
    epoch_credits: &[(u64, u64, u64)], 
    current_epoch: u64
) -> bool {
    !epoch_credits.iter().any(|(epoch, _, _)| {
        current_epoch.saturating_sub(*epoch) < 5
    })
}
#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    #[test]
    fn reference_recent_votes_are_acceptable() {
        let current = 100;
        let epoch_credits = vec![(100, 10, 0)]; // voted in current epoch
        assert!(acceptable_reference_epoch_credits(&epoch_credits, current));
    }

    #[test]
    fn reference_old_votes_are_not_acceptable() {
        let current = 100;
        let epoch_credits = vec![(98, 10, 0)]; // >1 epoch old
        assert!(!acceptable_reference_epoch_credits(&epoch_credits, current));
    }

    #[test]
    fn delinquent_if_all_votes_older_than_5_epochs() {
        let current = 100;
        let epoch_credits = vec![(94, 5, 0)]; // 6 epochs ago
        assert!(eligible_for_deactivate_delinquent(&epoch_credits, current));
    }

    #[test]
    fn not_delinquent_if_any_recent_vote_within_4_epochs() {
        let current = 100;
        let epoch_credits = vec![(97, 5, 0)]; // 3 epochs ago
        assert!(!eligible_for_deactivate_delinquent(&epoch_credits, current));
    }
}