#![allow(clippy::result_large_err)]
extern crate alloc;
use pinocchio::{
    account_info::AccountInfo,
    msg,
    program_error::ProgramError,
    sysvars::{clock::Clock, Sysvar},
};

use crate::{
    error::{to_program_error, StakeError},
    id,
    state::stake_state_v2::StakeStateV2,
    state::vote_state::vote_program_id,
};
use pinocchio::pubkey::Pubkey;
pub fn process_deactivate_delinquent(accounts: &[AccountInfo]) -> Result<(), ProgramError> {
    msg!("Instruction: DeactivateDelinquent");

    let mut iter = accounts.iter();
    let stake_ai           = iter.next().ok_or(ProgramError::NotEnoughAccountKeys)?;
    let delinquent_vote_ai = iter.next().ok_or(ProgramError::NotEnoughAccountKeys)?;
    let reference_vote_ai  = iter.next().ok_or(ProgramError::NotEnoughAccountKeys)?;

    let clock = Clock::get()?;
    const MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION: u64 = 5;
    // Optional: Only enforce owner check if your vote_program_id() is set to a real program id.
    let vote_pid = vote_program_id();
    if vote_pid != Pubkey::default() {
        if *reference_vote_ai.owner() != vote_pid || *delinquent_vote_ai.owner() != vote_pid {
            return Err(ProgramError::IncorrectProgramId);
        }
    }
    // 1) Reference validator must have a vote in EACH of the last N epochs
    {
       let data = reference_vote_ai.try_borrow_data()?;
        let ok = has_consecutive_votes_last_n_epochs(
            &data,
            clock.epoch,
            MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION,
        )?;
        if !ok {
            return Err(to_program_error(StakeError::InsufficientReferenceVotes));
        }
    } 

    // 2) Delinquent validator: last vote epoch <= (current_epoch - N)
   let delinquent_is_eligible = {
    let data = delinquent_vote_ai.try_borrow_data()?;
    let last = last_vote_epoch(&data)?;
    match last {
        None => true, // never voted => eligible
        Some(last_epoch) => {
            if let Some(min_epoch) =
                clock.epoch.checked_sub(MINIMUM_DELINQUENT_EPOCHS_FOR_DEACTIVATION)
            {
                last_epoch <= min_epoch
            } else {
                false
            }
        }
    }
};

    match stake_state(stake_ai)? {
        StakeStateV2::Stake(meta, mut stake, flags) => {
            // Must be the same vote account this stake is delegated to
            if stake.delegation.voter_pubkey != *delinquent_vote_ai.key() {
                return Err(to_program_error(StakeError::VoteAddressMismatch));
            }

            // if delinquent_is_eligible {
            //     stake.delegation.deactivation_epoch = clock.epoch.to_le_bytes();
            //     overwrite_stake_state(stake_ai, &StakeStateV2::Stake(meta, stake, flags))
             if delinquent_is_eligible {
                stake.deactivate(clock.epoch.to_le_bytes())
                    .map_err(to_program_error)?;
                overwrite_stake_state(stake_ai, &StakeStateV2::Stake(meta, stake, flags))
            } else {
                Err(to_program_error(
                    StakeError::MinimumDelinquentEpochsForDeactivationNotMet,
                ))
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


// fn has_vote_within_epochs(data: &[u8], current_epoch: u64, window: u64) -> Result<bool, ProgramError> {
//     if data.len() < 4 {
//         return Err(ProgramError::InvalidAccountData);
//     }
//     let mut n_bytes = [0u8; 4];
//     n_bytes.copy_from_slice(&data[0..4]);
//     let n = u32::from_le_bytes(n_bytes) as usize;

//     let mut off = 4usize;
//     for _ in 0..n {
//         // Need 24 bytes per triple
//         if off + 24 > data.len() {
//             return Err(ProgramError::InvalidAccountData);
//         }
//         // read epoch (first 8 bytes)
//         let mut e = [0u8; 8];
//         e.copy_from_slice(&data[off..off + 8]);
//         let epoch = u64::from_le_bytes(e);

//         // skip credits + prev_credits (we don't need them here)
//         off += 24;

//         if current_epoch.saturating_sub(epoch) <= window {
//             return Ok(true);
//         }
//     }
//     Ok(false)
// }

// NEW: strict consecutive reference check (matches acceptable_reference_epoch_credits)
// Require a vote in EACH of the last `n` epochs: current_epoch, current_epoch-1, ..., current_epoch-(n-1)
fn has_consecutive_votes_last_n_epochs(
    data: &[u8],
    current_epoch: u64,
    n: u64,
) -> Result<bool, ProgramError> {
    if data.len() < 4 { return Err(ProgramError::InvalidAccountData); }
    let mut n_bytes = [0u8; 4];
    n_bytes.copy_from_slice(&data[0..4]);
    let count = u32::from_le_bytes(n_bytes) as usize;

    // Need enough entries to possibly cover n epochs
    let need = 4usize + count.saturating_mul(24);
    if data.len() < need { return Err(ProgramError::InvalidAccountData); }

    // Bitset for the last n epochs (n <= 64 by your MAX_EPOCH_CREDITS bound)
    let mut seen: u64 = 0;
    let mut off = 4usize;
    for _ in 0..count {
        let mut e = [0u8; 8];
        e.copy_from_slice(&data[off..off + 8]);
        let epoch = u64::from_le_bytes(e);
        off += 24; // skip credits + prev_credits too

        if epoch <= current_epoch {
            let delta = current_epoch.saturating_sub(epoch);
            if delta < n {
                // mark that epoch as seen
                seen |= 1u64 << delta;
                if seen.count_ones() as u64 == n {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

// Last recorded vote epoch in the buffer (or None if empty)
fn last_vote_epoch(data: &[u8]) -> Result<Option<u64>, ProgramError> {
    if data.len() < 4 { return Err(ProgramError::InvalidAccountData); }
    let mut n_bytes = [0u8; 4];
    n_bytes.copy_from_slice(&data[0..4]);
    let count = u32::from_le_bytes(n_bytes) as usize;
    if count == 0 { return Ok(None); }
    let need = 4usize + count.saturating_mul(24);
    if data.len() < need { return Err(ProgramError::InvalidAccountData); }
    let off = 4 + (count - 1) * 24;
    let mut e = [0u8; 8];
    e.copy_from_slice(&data[off..off + 8]);
    Ok(Some(u64::from_le_bytes(e)))
}
#[cfg(test)]
mod tests {
    use super::*;

    fn build_epoch_credits_bytes(list: &[(u64, u64, u64)]) -> alloc::vec::Vec<u8> {
        use alloc::vec::Vec;
        let mut out = Vec::with_capacity(4 + list.len() * 24);
        out.extend_from_slice(&(list.len() as u32).to_le_bytes());
        for &(e, c, p) in list {
            out.extend_from_slice(&e.to_le_bytes());
            out.extend_from_slice(&c.to_le_bytes());
            out.extend_from_slice(&p.to_le_bytes());
        }
        out
    }

   #[test]
fn reference_has_all_last_n_epochs() {
    // current = 100, need epochs 100..=96 present
    let current = 100;
    let bytes = build_epoch_credits_bytes(&[
        (96, 1, 0),
        (97, 2, 1),
        (98, 3, 2),
        (99, 4, 3),
        (100, 5, 4),
    ]);
    assert!(has_consecutive_votes_last_n_epochs(&bytes, current, 5).unwrap());
}

#[test]
fn reference_missing_one_epoch_fails() {
    // Missing 98 in the last 5 => should fail
    let current = 100;
    let bytes = build_epoch_credits_bytes(&[
        (96, 1, 0),
        (97, 2, 1),
        //(98 missing)
        (99, 4, 3),
        (100, 5, 4),
    ]);
    assert!(!has_consecutive_votes_last_n_epochs(&bytes, current, 5).unwrap());
}

#[test]
fn delinquent_if_last_vote_older_than_n() {
    // current=100, N=5 => min_epoch = 95
    // last=94 => 94 <= 95 => eligible (delinquent)
    let current = 100;
    let bytes = build_epoch_credits_bytes(&[(94, 5, 0)]);
    let last = last_vote_epoch(&bytes).unwrap();
    assert_eq!(last, Some(94));
    let min_epoch = current - 5;
    assert!(last.unwrap() <= min_epoch);
}

#[test]
fn not_delinquent_if_last_vote_within_n() {
    // current=100, N=5 => min_epoch=95
    // last=97 => 97 > 95 => NOT delinquent
    let current = 100;
    let bytes = build_epoch_credits_bytes(&[(97, 5, 0)]);
    let last = last_vote_epoch(&bytes).unwrap();
    assert_eq!(last, Some(97));
    let min_epoch = current - 5;
    assert!(!(last.unwrap() <= min_epoch));
}
}
