#![allow(clippy::result_large_err)]

use alloc::vec::Vec; // for the small unit tests at the bottom

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
    vote_state::VoteState,
};

pub fn process_deactivate_delinquent(accounts: &[AccountInfo]) -> Result<(), ProgramError> {
    msg!("Instruction: DeactivateDelinquent");

    let mut iter = accounts.iter();
    let stake_ai            = iter.next().ok_or(ProgramError::NotEnoughAccountKeys)?;
    let delinquent_vote_ai  = iter.next().ok_or(ProgramError::NotEnoughAccountKeys)?;
    let reference_vote_ai   = iter.next().ok_or(ProgramError::NotEnoughAccountKeys)?;

    let clock = Clock::get()?;

    let delinquent_vs = VoteState::from_account_info(delinquent_vote_ai)?;
    let reference_vs  = VoteState::from_account_info(reference_vote_ai)?;

    if !acceptable_reference_epoch_credits(reference_vs.epoch_credits_as_slice(), clock.epoch) {
        return Err(StakeError::InsufficientReferenceVotes.into());
    }

    match stake_state(stake_ai)? {
        StakeStateV2::Stake(mut meta, mut stake, flags) => {
            if stake.delegation.voter_pubkey != *delinquent_vote_ai.key() {
                return Err(StakeError::VoteAddressMismatch.into());
            }

            if eligible_for_deactivate_delinquent(delinquent_vs.epoch_credits_as_slice(), clock.epoch)
            {
                stake.delegation.deactivation_epoch = clock.epoch;
            
                overwrite_stake_state(stake_ai, &StakeStateV2::Stake(meta, stake, flags))
            } else {
                Err(StakeError::MinimumDelinquentEpochsForDeactivationNotMet.into())
            }
        }
        _ => Err(ProgramError::InvalidAccountData),
    }
}


fn stake_state(ai: &AccountInfo) -> Result<StakeStateV2, ProgramError> {
    if ai.owner() != &id() {
        return Err(ProgramError::InvalidAccountOwner);
    }
    let data = ai.try_borrow_data()?;
    StakeStateV2::deserialize(&data)
}

fn overwrite_stake_state(ai: &AccountInfo, s: &StakeStateV2) -> Result<(), ProgramError> {
    if ai.owner() != &id() || !ai.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }
    let mut data = ai.try_borrow_mut_data()?;
    s.serialize(&mut data)
}

fn acceptable_reference_epoch_credits(ecs: &[(u64, u64, u64)], current_epoch: u64) -> bool {
    ecs.iter().any(|(epoch, _, _)| current_epoch.saturating_sub(*epoch) <= 1)
}

fn eligible_for_deactivate_delinquent(ecs: &[(u64, u64, u64)], current_epoch: u64) -> bool {
    !ecs.iter().any(|(epoch, _, _)| current_epoch.saturating_sub(*epoch) < 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_recent_votes_are_acceptable() {
        let current = 100;
        let ecs = vec![(100, 10, 0)]; // voted in current epoch
        assert!(acceptable_reference_epoch_credits(&ecs, current));
    }

    #[test]
    fn reference_old_votes_are_not_acceptable() {
        let current = 100;
        let ecs = vec![(98, 10, 0)]; // >1 epoch old
        assert!(!acceptable_reference_epoch_credits(&ecs, current));
    }

    #[test]
    fn delinquent_if_all_votes_older_than_5_epochs() {
        let current = 100;
        let ecs = vec![(94, 5, 0)]; // 6 epochs ago
        assert!(eligible_for_deactivate_delinquent(&ecs, current));
    }

    #[test]
    fn not_delinquent_if_any_recent_vote_within_4_epochs() {
        let current = 100;
        let ecs = vec![(97, 5, 0)]; // 3 epochs ago
        assert!(!eligible_for_deactivate_delinquent(&ecs, current));
    }
}