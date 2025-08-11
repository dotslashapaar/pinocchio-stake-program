
use pinocchio::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};

use crate::state::accounts::Authorized;

// I'm using byte arrays instead of regular numbers to match how Solana stores them
// Little-endian means the smallest byte comes first (like 0x1234 becomes [34, 12, 00, 00...])
pub type UnixTimestamp = [u8; 8]; // This will hold timestamps (i64 converted to bytes)
pub type Epoch         = [u8; 8]; // This will hold epoch numbers (u64 converted to bytes)

#[repr(C)]  // This makes Rust lay out the struct like C would (predictable memory layout)
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct Lockup {
    // When can the user withdraw? (unless they have special permission)
    pub unix_timestamp: UnixTimestamp, // Storing time as 8 bytes
    // What epoch does this unlock? 
    pub epoch: Epoch,                   // Storing epoch number as 8 bytes
    // Special person who can override the lockup
    pub custodian: Pubkey,
}

impl Lockup {
    #[inline]  // Tells compiler to insert this code directly where it's called (faster)
    pub const fn size() -> usize {
        // Getting the size of our struct in bytes
        core::mem::size_of::<Lockup>()
    }

    /// Convenience constructor from native numbers.
    #[inline]
    pub fn new(unix_timestamp: i64, epoch: u64, custodian: Pubkey) -> Self {
        // Creating a new Lockup, converting numbers to byte arrays
        Self {
            unix_timestamp: unix_timestamp.to_le_bytes(), // Convert i64 to 8 bytes
            epoch: epoch.to_le_bytes(),                   // Convert u64 to 8 bytes  
            custodian,
        }
    }

    /// Is the lockup still active for the given (host) time/epoch?
    #[inline]
    pub fn is_active(&self, current_timestamp: i64, current_epoch: u64) -> bool {
        // Check if we're still locked
        // First convert our bytes back to numbers, then compare
        current_timestamp < i64::from_le_bytes(self.unix_timestamp)  // Still before unlock time?
            || current_epoch < u64::from_le_bytes(self.epoch)        // Still before unlock epoch?
    }

    // Helper to read a Lockup directly from account data
    // WARNING: This assumes the account contains ONLY a Lockup at the start!
    #[inline]
    pub fn get_account_info(account: &AccountInfo) -> Result<&Self, ProgramError> {
        // Make sure account has enough data for a Lockup
        if account.data_len() < Self::size() {
            return Err(ProgramError::InvalidAccountData);
        }
        // Make sure our program owns this account
        if account.owner() != &crate::ID {
            return Err(ProgramError::IncorrectProgramId);
        }
        // UNSAFE: Cast the raw bytes directly to our Lockup struct
        // This works because we used repr(C) for predictable layout
        Ok(unsafe { &*(account.borrow_data_unchecked().as_ptr() as *const Self) })
    }

    // Same as above but for mutable access
    #[inline]
    pub fn get_account_info_mut(account: &AccountInfo) -> Result<&mut Self, ProgramError> {
        // Check size
        if account.data_len() < Self::size() {
            return Err(ProgramError::InvalidAccountData);
        }
        // Must be writable to modify
        if !account.is_writable() {
            return Err(ProgramError::InvalidAccountData);
        }
        // Must be owned by our program
        if account.owner() != &crate::ID {
            return Err(ProgramError::IncorrectProgramId);
        }
        // UNSAFE: Get mutable reference to the data as our struct
        Ok(unsafe { &mut *(account.borrow_mut_data_unchecked().as_ptr() as *mut Self) })
    }
}

#[repr(C)]  // Again, C-style memory layout for compatibility
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct Meta {
    // How much SOL to keep for rent (stored as bytes like above)
    pub rent_exempt_reserve: [u8; 8], // This is u64 in little-endian bytes
    // Who can control this stake account
    pub authorized: Authorized,
    // The lockup rules
    pub lockup: Lockup,
}

impl Meta {
    #[inline]
    pub fn size() -> usize {
        // How many bytes does Meta struct take up?
        core::mem::size_of::<Meta>()
    }

    // Read Meta directly from account (assumes Meta is at start of account)
    // NOTE: Usually stake accounts have more than just Meta, so be careful!
    #[inline]
    pub fn get_account_info(account: &AccountInfo) -> Result<&Self, ProgramError> {
        // Need enough bytes for a Meta struct
        if account.data_len() < Self::size() {
            return Err(ProgramError::InvalidAccountData);
        }
        // Check it's owned by our stake program
        if account.owner() != &crate::ID {
            return Err(ProgramError::IncorrectProgramId);
        }
        // UNSAFE: Interpret the bytes as our Meta struct
        // This is fast but dangerous - make sure the data really is a Meta!
        Ok(unsafe { &*(account.borrow_data_unchecked().as_ptr() as *const Self) })
    }

    // Mutable version - same idea but for writing
    #[inline]
    pub fn get_account_info_mut(account: &AccountInfo) -> Result<&mut Self, ProgramError> {
        // Check we have enough data
        if account.data_len() < Self::size() {
            return Err(ProgramError::InvalidAccountData);
        }
        // Must be writable
        if !account.is_writable() {
            return Err(ProgramError::InvalidAccountData);
        }
        // Must be our program's account
        if account.owner() != &crate::ID {
            return Err(ProgramError::IncorrectProgramId);
        }
        // UNSAFE: Get mutable pointer to modify the Meta
        // I'm treating raw bytes as our struct - risky but fast!
        Ok(unsafe { &mut *(account.borrow_mut_data_unchecked().as_ptr() as *mut Self) })
    }
}