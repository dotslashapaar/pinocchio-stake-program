use pinocchio::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::clock::{Clock},
};

// Imports from crate
use crate::{
    id,
    state::accounts::{AuthorizeWithSeedData, Authorized, Lockup, StakeAuthorize},
};

pub fn process_authorize_checked_with_seed(
    accounts: &[AccountInfo],
    instruction_data: AuthorizeWithSeedData,
) -> ProgramResult {
    // 4 accounts are required: stake account, old authority base, clock info, new authority
    if accounts.len() < 4 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    let [stake_account, old_authority_base, clock_info, new_authority, _remaining @ ..] = accounts
    else {
        return Err(ProgramError::InvalidAccountData);
    };

    // Optional lockup authority
    let option_lockup_authority = if accounts.len() > 4 {
        // Checks for an optional 5th account (lockup authority/custodian)
        Some(&accounts[4])
    } else {
        None
    };

    //Ensures the stake account can be modified
    if !stake_account.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }

    // Ensures the stake account is owned by the correct program
    if stake_account.owner() != &id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    // Ensures the new authority is a signer
    if !new_authority.is_signer() {
        
        return Err(ProgramError::MissingRequiredSignature);
    }
  
     // Ensures the old authority base is a signer
    if !old_authority_base.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Verify custodian is a signer if provided
    if let Some(custodian_info) = option_lockup_authority {
        if !custodian_info.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }
    }

     // Loads the current time/epoch from the clock sysvar
    let clock = Clock::from_account_info(clock_info)?;

    // Derive the expected old authority key
    let seed = &instruction_data.authority_seed[..instruction_data.authority_seed_len as usize];
    let derived_key = Pubkey::create_with_seed(
        old_authority_base.key(),
        seed,
        &instruction_data.authority_owner,
    )?;

    let mut authorized = Authorized::get_account_info_mut(stake_account)?;
    let lockup = Lockup::from_account_info(stake_account)?;

    // Checks if the stake is time-locked
    if lockup.is_active(clock.unix_timestamp, clock.epoch) {
        let custodian_authorized =
            option_lockup_authority.map_or(false, |c| c.key() == &lockup.custodian);
        if !custodian_authorized {
            return Err(ProgramError::LockupInForce);
        }
    }

    // Checks if the derived key is the current staker or withdrawer;
    // if valid, updates to the new authority’s key.
    match instruction_data.stake_authorize {
        StakeAuthorize::Staker => {
            if !authorized.is_staker(&derived_key) {
                return Err(ProgramError::InvalidAuthority);
            }
            authorized.staker = *new_authority.key();
        }
        StakeAuthorize::Withdrawer => {
            if !authorized.is_withdrawer(&derived_key) {
                return Err(ProgramError::InvalidAuthority);
            }
            authorized.withdrawer = *new_authority.key();
        }
    }

    Ok(())
}
