use crate::state::accounts::Authorized;
use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::clock::{Clock, Epoch, UnixTimestamp},
};

#[repr(C)]
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct Lockup {
    /// UnixTimestamp at which this stake will allow withdrawal, unless
    /// the transaction is signed by the custodian
    pub unix_timestamp: UnixTimestamp, // i64
    /// Epoch height at which this stake will allow withdrawal, unless
    /// the transaction is signed by the custodian
    pub epoch: Epoch,                  // u64
    /// Custodian whose signature exempts the operation from lockup constraints
    pub custodian: Pubkey,
}

#[repr(C)]
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct Meta {
    pub rent_exempt_reserve: [u8; 8],
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
        }
        if !account.is_writable() {
            return Err(ProgramError::InvalidAccountData);
        }
        if account.owner() != &crate::ID {
            return Err(ProgramError::IncorrectProgramId);
        }
        Ok(unsafe { &*(account.borrow_data_unchecked().as_ptr() as *const Self) })
    }

    pub fn get_account_info_mut(account: &AccountInfo) -> Result<&mut Self, ProgramError> {
        if account.data_len() < core::mem::size_of::<Meta>() {
            return Err(ProgramError::InvalidAccountData);
        }
        if !account.is_writable() {
            return Err(ProgramError::InvalidAccountData);
        }
        if account.owner() != &crate::ID {
            return Err(ProgramError::IncorrectProgramId);
        }
        Ok(unsafe { &mut *(account.borrow_data_unchecked().as_ptr() as *mut Self) })
    }
}

impl Lockup {
    pub const fn size() -> usize {
        core::mem::size_of::<Lockup>()
    }

    /// Create a new lockup (integers, no byte encoding)
    pub fn new(unix_timestamp: i64, epoch: Epoch, custodian: Pubkey) -> Self {
        Self {
            unix_timestamp,
            epoch,
            custodian,
        }
    }

    /// Check if lockup is active at the given wall time and epoch
    pub fn is_active(&self, current_timestamp: i64, current_epoch: u64) -> bool {
        // In force if *either* constraint hasn't passed yet (0 means "no constraint")
        let time_in_force  = self.unix_timestamp != 0 && current_timestamp < self.unix_timestamp;
        let epoch_in_force = self.epoch          != 0 && current_epoch   < self.epoch;
        time_in_force || epoch_in_force
    }

    pub fn get_account_info(account: &AccountInfo) -> Result<&Self, ProgramError> {
        if account.data_len() < Self::size() {
            return Err(ProgramError::InvalidAccountData);
        }
        if account.owner() != &crate::ID {
            return Err(ProgramError::IncorrectProgramId);
        }
        Ok(unsafe { &*(account.borrow_data_unchecked().as_ptr() as *const Self) })
    }

    pub fn get_account_info_mut(account: &AccountInfo) -> Result<&mut Self, ProgramError> {
        if account.data_len() < Self::size() {
            return Err(ProgramError::InvalidAccountData);
        }
        if !account.is_writable() {
            return Err(ProgramError::InvalidAccountData);
        }
        if account.owner() != &crate::ID {
            return Err(ProgramError::IncorrectProgramId);
        }
        Ok(unsafe { &mut *(account.borrow_mut_data_unchecked().as_ptr() as *mut Self) })
    }

    /// Custodian signature bypasses lockup
    #[inline(always)]
    pub fn is_in_force(&self, clock: &Clock, custodian_signer: Option<&Pubkey>) -> bool {
        // Bypass if the configured custodian signed
        if let Some(sig) = custodian_signer {
            if *sig == self.custodian {
                return false;
            }
        }

        let time_in_force  = self.unix_timestamp != 0 && clock.unix_timestamp < self.unix_timestamp;
        let epoch_in_force = self.epoch          != 0 && clock.epoch          < self.epoch;

        time_in_force || epoch_in_force
    }
}
