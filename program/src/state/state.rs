use crate::helpers::{bytes_to_u64, Epoch};
use crate::state::accounts::Authorized;
use pinocchio::{
    account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey, sysvars::clock::Clock,
};
#[repr(C)]
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct Lockup {
    /// UnixTimestamp at which this stake will allow withdrawal, unless the
    ///   transaction is signed by the custodian
    pub unix_timestamp: i64,
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
impl Lockup {
    pub const fn size() -> usize {
        core::mem::size_of::<Lockup>()
    }

    /// Create a new lockup
    pub fn new(unix_timestamp: i64, epoch: Epoch, custodian: Pubkey) -> Self {
        Self {
            unix_timestamp,
            epoch,
            custodian,
        }
    }

    /// Check if the lockup is active for the given timestamp and epoch
    pub fn is_active(&self, current_timestamp: i64, current_epoch: u64) -> bool {
        current_timestamp < self.unix_timestamp || current_epoch < bytes_to_u64(self.epoch)
    }

    pub fn get_account_info(account: &AccountInfo) -> Result<&Self, ProgramError> {
        if account.data_len() < Self::size() {
            return Err(ProgramError::InvalidAccountData);
        };

        if account.owner() != &crate::ID {
            return Err(ProgramError::IncorrectProgramId);
        };

        return Ok(unsafe { &*(account.borrow_data_unchecked().as_ptr() as *const Self) });
    }

    pub fn get_account_info_mut(account: &AccountInfo) -> Result<&mut Self, ProgramError> {
        if account.data_len() < Self::size() {
            return Err(ProgramError::InvalidAccountData);
        };

        if !account.is_writable() {
            return Err(ProgramError::InvalidAccountData);
        };

        if account.owner() != &crate::ID {
            return Err(ProgramError::IncorrectProgramId);
        };

        return Ok(unsafe { &mut *(account.borrow_mut_data_unchecked().as_ptr() as *mut Self) });
    }
    #[inline(always)]
    pub fn is_in_force(&self, clock: &Clock, custodian_signer: Option<&[u8; 32]>) -> bool {
        // If a custodian is configured on the lockup and that custodian signed, lockup is bypassed
        let has_custodian = self.custodian != [0u8; 32];
        if has_custodian {
            if let Some(sig) = custodian_signer {
                if *sig == self.custodian {
                    return false; // bypassed by custodian
                }
            }
        }

        // Decode LE-encoded fields
        let unix_ts = self.unix_timestamp; // already i64
        let epoch_lo = bytes_to_u64(self.epoch); // [u8;8] -> u64

        // Lockup remains in force if *either* constraint hasn't passed yet.
        let time_in_force = unix_ts != 0 && clock.unix_timestamp < unix_ts;
        let epoch_in_force = epoch_lo != 0 && clock.epoch < epoch_lo;

        time_in_force || epoch_in_force
    }
}
