use pinocchio::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    program_error::ProgramError,
    sysvars::clock::{Clock},
};

// Imports from crate
use crate::{
    id,
    state::accounts::{Authorized, Lockup, Stake},
    
};

pub fn process_deactivate(accounts: &[AccountInfo]) -> ProgramResult {
    // 2 accounts are required: stake account, clock info
    if accounts.len() < 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    // basically this Unpacks accounts
    let [stake_account, clock_info, _remaining @ ..] = accounts else {
        return Err(ProgramError::InvalidAccountData);
    };

    // Checks for an optional 3rd account (lockup authority/custodian).
    let option_lockup_authority = if accounts.len() > 2 { Some(&accounts[2]) } else { None };

    // Ensures the stake account can be modified
    if !stake_account.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }

    // Ensures the stake account is owned by the correct program
    if stake_account.owner() != &id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    // Assume the first account after stake and clock is the staker
    let staker = if accounts.len() > 2 {
        &accounts[2]
    } else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Ensures the staker is a signer
    if !staker.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Verify custodian is a signer if provided
    if let Some(custodian_info) = option_lockup_authority {
        if !custodian_info.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }
    }

    let clock = Clock::from_account_info(clock_info)?;
    let mut stake = Stake::get_account_info_mut(stake_account)?;
    let lockup = Lockup::get_account_info(stake_account)?;
    let authorized = Authorized::get_account_info(stake_account)?;

    // Check if the staker is authorized
    if !authorized.is_staker(staker.key()) {
        return Err(ProgramError::InvalidAuthority );
    }

    // Checks if the stake is time-locked; requires a valid custodian if active 
    if lockup.is_active(clock.unix_timestamp, clock.epoch) {
        let custodian_authorized = option_lockup_authority
            .map_or(false, |c| c.key() == &lockup.custodian);
        if !custodian_authorized {
            return Err(ProgramError::LockupInForce );
        }
    }

    // Check if stake is fully activated like is delegation started
    if !stake.delegation.is_fully_activated(clock.epoch) {
        return Err(ProgramError::InvalidStakeState );
    }

    // Check if already deactivated
    if stake.delegation.deactivation_epoch != u64::MAX {
        return Err(ProgramError::AlreadyDeactivated );
    }

    // Sets the deactivation epoch to the current epoch,
    // marking the stake as deactivated.
    stake.delegation.deactivation_epoch = clock.epoch;

    Ok(())
}
