use crate::helpers::constant::*;
use crate::state::stake_state_v2::StakeStateV2;
use crate::state::vote_state::VoteState;
use crate::state::{Meta, delegation::{Stake, Delegation}};
use crate::error::{StakeError, to_program_error};
use crate::ID;
use pinocchio::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};

// helper for stake amount validation
pub struct ValidatedDelegatedInfo {
    pub stake_amount: u64,
}

// wrapper for epoch to pass around
pub struct StakeHistorySysvar(pub u64);
pub enum ErrorCode {
    TOOMANYSIGNERS = 0x1,
}

// almost all native stake program processors accumulate every account signer
// they then defer all signer validation to functions on Meta or Authorized
// this results in an instruction interface that is much looser than the one documented
// to avoid breaking backwards compatibility, we do the same here
// in the future, we may decide to tighten the interface and break badly formed transactions
pub fn collect_signers(
    accounts: &[AccountInfo],
    array_of_signers: &mut [Pubkey; MAXIMUM_SIGNERS],
) -> Result<usize, ProgramError> {
    let mut len_of_signers = 0;

    for account in accounts {
        if account.is_signer() {
            if len_of_signers < MAXIMUM_SIGNERS {
                array_of_signers[len_of_signers] = *account.key();
                len_of_signers += 1;
            } else {
                return Err(ProgramError::Custom(ErrorCode::TOOMANYSIGNERS as u32));
            }
        }
    }

    Ok(len_of_signers)
}

pub fn next_account_info<'a, I: Iterator<Item = &'a AccountInfo>>(
    iter: &mut I,
) -> Result<&'a AccountInfo, ProgramError> {
    iter.next().ok_or(ProgramError::NotEnoughAccountKeys)
}

// fn get_stake_state(stake_account_info: &AccountInfo) -> Result<StakeStateV2, ProgramError> {
//     if *stake_account_info.owner() != ID {
//         return Err(ProgramError::InvalidAccountOwner);
//     }
// }

// returns a deserialized vote state from raw account data
pub fn get_vote_state(vote_account_info: &AccountInfo) -> Result<VoteState, ProgramError> {
    // enforce account is large enough
    let data = unsafe { vote_account_info.borrow_data_unchecked() };
    if data.len() < core::mem::size_of::<VoteState>() {
        return Err(ProgramError::InvalidAccountData);
    }

    let vote_state = unsafe { &*(data.as_ptr() as *const VoteState) };
    Ok(vote_state.clone())
}

// load stake state from account
pub fn get_stake_state(stake_account_info: &AccountInfo) -> Result<StakeStateV2, ProgramError> {
    if *stake_account_info.owner() != ID {
        return Err(ProgramError::InvalidAccountOwner);
    }

    let data = unsafe { stake_account_info.borrow_data_unchecked() };
    StakeStateV2::deserialize(&data)
}

// write stake state back into account
pub fn set_stake_state(
    stake_account_info: &AccountInfo,
    stake_state: &StakeStateV2,
) -> Result<(), ProgramError> {
    let mut data = unsafe { stake_account_info.borrow_mut_data_unchecked() };
    stake_state.serialize(&mut data)?;
    Ok(())
}

// compute stake amount = lamports - rent exempt reserve
pub fn validate_delegated_amount(
    stake_account_info: &AccountInfo,
    meta: &Meta,
) -> Result<ValidatedDelegatedInfo, ProgramError> {
    let stake_amount = stake_account_info
        .lamports()
        .checked_sub(meta.rent_exempt_reserve)
        .ok_or(StakeError::InsufficientFunds)
        .map_err(to_program_error)?;

    Ok(ValidatedDelegatedInfo { stake_amount })
}

// create new stake object from inputs
pub fn new_stake(
    stake_amount: u64,
    vote_pubkey: &Pubkey,
    vote_state: &VoteState,
    activation_epoch: u64,
) -> Stake {
    let mut stake = Stake::default();
    stake.delegation = Delegation::new(vote_pubkey, stake_amount, activation_epoch);
    stake.set_credits_observed(vote_state.credits());
    stake
}

// modify existing stake object with updated delegation
pub fn redelegate_stake(
    stake: &mut Stake,
    stake_amount: u64,
    vote_pubkey: &Pubkey,
    vote_state: &VoteState,
    clock_epoch: u64,
    _stake_history: &StakeHistorySysvar,
) -> Result<(), ProgramError> {
    stake.delegation.voter_pubkey = *vote_pubkey;
    stake.delegation.set_stake_amount(stake_amount);
    stake.delegation.activation_epoch = clock_epoch;
    stake.set_credits_observed(vote_state.credits());
    Ok(())
}
