use pinocchio::{
    account_info::AccountInfo, program_error::ProgramError, pubkey::{self, Pubkey}, sysvars::{clock::Clock, rent::Rent, Sysvar}, ProgramResult, 
};

use pinocchio_system::instructions::CreateAccount;

use crate::state::accounts::{
    Meta, 
    SetLockupData, 
    Authorized, 
    Lockup
};

pub fn process_set_lockup(accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult {
    //Unpacking accounts
    let [stake_account, lockup_account, authority, system_program, _remaining @..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    //Validating instruction data size
    if instruction_data.len() < core::mem::size_of::<SetLockupData>() {
        return Err(ProgramError::InvalidInstructionData);
    };

    //Deriving the expected lockup_account PDA
    let (lockup_pda, bump) = pubkey::find_program_address(
        &[b"lockup", stake_account.key().as_ref()],
        &crate::ID
    );

    if lockup_account.key() != &lockup_pda {
        return Err(ProgramError::InvalidSeeds);
    };

    //Getting the stake account meta
    let mut meta = Meta::get_account_info_mut(stake_account)?;

    //Getting the authorized account
    let authorized = Authorized::get_account_info(authority)?;

    //Checking if the authority is either the staker or withdrawer
    if !authorized.is_staker(authority.key()) && !authorized.is_withdrawer(authority.key()) {
        return Err(ProgramError::InvalidAccountData);
    };

    //Parsing the instruction data
    let set_lockup_data = SetLockupData::instruction_data(instruction_data);

    //Getting current time and epoch
    let clock = Clock::get()?;
    let current_timestamp = clock.unix_timestamp;
    let current_epoch = clock.epoch;

    //Checking weather the lockup_account is needed to be created
    if lockup_account.owner() != crate::ID {
        //Initializing an lockup account

        let rent = Rent::get()?;
        let lamports = rent.minimum_balance(Lockup::size());

        //Creating lockup account via CPI to systemProgram
        CreateAccount {
            from: authority,
            to: lockup_account,
            lamports,
            space: Lockup::size(),
            owner: &crate::ID
        }.invoke();

        //Initialize the lockup_account with provided data
        let mut lockup = Lockup::get_account_info_mut(lockup_account)?;

        lockup.unix_timestamp = set_lockup_data.unix_timestamp.unwrap_or(0);
        lockup.epoch = set_lockup_data.epoch.unwrap_or(0);
        lockup.custodian = set_lockup_data.custodian.unwrap_or(Pubkey::default());

        //Update meta to reference to new lockup account
        meta.lockup = *lockup_account;
    } else {

        // just updating the lockup_account (expected the lockup_account to already exist)

        let mut current_lockup = Lockup::get_account_info_mut(lockup_account)?;

        //Check if we can modify the lockup (only if lockup is not currently active OR authority is custodian)
        let can_modify = !current_lockup.is_active(current_timestamp, current_epoch) || authority.key() == &current_lockup.custodian;

        if !can_modify {
            return Err(ProgramError::InvalidAccountData);
        };

        //Updating account lockup parameters
        if let Some(unix_timestamp) = set_lockup_data.unix_timestamp {
            //Onlt allow setting a timestamp in the future or making it more restrictive
            if unix_timestamp >= current_lockup.unix_timestamp {
                current_lockup.unix_timestamp = unix_timestamp
            };
        };

        if let Some(epoch) = set_lockup_data.epoch {
            //Only allow setting an epoch in the future or making it more restrictive
            if epoch >= current_lockup.epoch {
                current_lockup.epoch = epoch
            };
        };

        if let Some(custodian) = set_lockup_data.custodian {
            current_lockup.custodian = custodian
        };

        // Update meta to reference the lockup account (in case it changed)
        meta.lockup = *lockup_account
    }

}