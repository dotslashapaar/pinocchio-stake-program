use crate::state::accounts::Authorized;
use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::clock::{Epoch, UnixTimestamp},
};
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct Lockup {
    /// UnixTimestamp at which this stake will allow withdrawal, unless the
    ///   transaction is signed by the custodian
    pub unix_timestamp: UnixTimestamp,
    /// epoch height at which this stake will allow withdrawal, unless the
    ///   transaction is signed by the custodian
    pub epoch: Epoch,
    /// custodian signature on a transaction exempts the operation from
    ///  lockup constraints
    pub custodian: Pubkey,
}

#[repr(C)]
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct Meta {
    pub rent_exempt_reserve: [u8; 8], // Using array for fixed size
    pub authorized: Authorized,
    pub lockup: Lockup,
}
impl Meta {
    pub fn size() -> usize {
        core::mem::size_of::<Meta>()
    }

    pub fn get_account_info(account: &AccountInfo) -> Result<&Self, ProgramError> {
        if account.data_len() < core::mem::size_of::<Meta>() {
            return Err(ProgramError::InvalidAccountData);
        };

        if !account.is_writable() {
            return Err(ProgramError::InvalidAccountData);
        }

        if account.owner() != &crate::ID {
            return Err(ProgramError::IncorrectProgramId);
        };

        return Ok(unsafe { &*(account.borrow_data_unchecked().as_ptr() as *const Self) });
    }

    pub fn get_account_info_mut(account: &AccountInfo) -> Result<&mut Self, ProgramError> {
        if account.data_len() < core::mem::size_of::<Meta>() {
            return Err(ProgramError::InvalidAccountData);
        };

        if !account.is_writable() {
            return Err(ProgramError::InvalidAccountData);
        }

        if account.owner() != &crate::ID {
            return Err(ProgramError::IncorrectProgramId);
        };

        return Ok(unsafe { &mut *(account.borrow_data_unchecked().as_ptr() as *mut Self) });
    }
}
