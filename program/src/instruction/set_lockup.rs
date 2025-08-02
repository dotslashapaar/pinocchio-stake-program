use pinocchio::{
    account_info::AccountInfo, program_error::ProgramError, sysvars::clock::Clock
};

use crate::state::accounts::{
    Meta, 
    SetLockupData, 
    Authorized, 
    Lockup
};

pub fn process_set_lockup(
    accounts: &[AccountInfo],
    data: &[u8],
    clock: &Clock
) -> Result<(), ProgramError> {
    let [stake_account, signer_account, authorized_account, lockup_account] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let meta = Meta::get_account_info(stake_account)?;
    let authorized = Authorized::get_account_info(authorized_account);
    let lockup = Lockup::get_account_info(lockup_account)?;

    if *authorized_account.key() != meta.authorized {
        return Err(ProgramError::InvalidAccountData);
    }

    if *lockup_account.key() != meta.lockup {
        return Err(ProgramError::InvalidAccountData);
    }

    let withdrawer = authorized.withdrawer;
    let custodian = lockup.custodian;
    let signer_pubkey = *signer_account.key();

    let lockup_active = lockup.is_active(clock.unix_timestamp, clock.epoch);

    if lockup_active {
        if signer_pubkey != custodian || !signer_account.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }
    } else {
        if signer_pubkey != withdrawer || !signer_account.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }
    }

    let instruction_data = SetLockupData::instruction_data(data);

    if let Some(time_stamp) = instruction_data.unix_timestamp {
        lockup.unix_timestamp = time_stamp;
    }
    if let Some(epoch) = instruction_data.epoch {
        lockup.epoch = epoch;
    }
    if let Some(new_custodian) = instruction_data.custodian {
        lockup.custodian = new_custodian;
    }

    Ok(())
}

// =================== Testing process_set_lockup ======================

// #[cfg(test)]
// pub mod testing {
//     use mollusk_svm::{Mollusk};

//     #[test]
//     fn test_process_set_lockup() {
//         let mollusk = Mollusk::new(&crate::ID, )
//     }
// }