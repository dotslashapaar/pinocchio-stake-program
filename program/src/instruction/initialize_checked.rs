#![allow(clippy::result_large_err)]

  
  use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    sysvars::rent::Rent,
    ProgramResult,
};

use crate::{ state::state::Lockup};
use crate::instruction::initialize::do_initialize;
use crate::state::*;

pub fn process_initialize_checked(accounts: &[AccountInfo]) -> ProgramResult {

        // native asserts: 4 accounts (1 sysvar)

    let [stake_account_info, rent_info,stake_authority_info,withdraw_authority_info, _rest @ ..] = accounts else{
        return Err(ProgramError::NotEnoughAccountKeys);
    };


        let rent = &Rent::from_account_info(rent_info)?;

        if !withdraw_authority_info.is_signer(){
            return Err(ProgramError::MissingRequiredSignature);
        }

        let authorized = Authorized {
            staker: *stake_authority_info.key(),
            withdrawer: *withdraw_authority_info.key(),
        };

        // `get_stake_state()` is called unconditionally, which checks owner
        do_initialize(stake_account_info, authorized, Lockup::default(), rent)?;

        Ok(())
    }
    

