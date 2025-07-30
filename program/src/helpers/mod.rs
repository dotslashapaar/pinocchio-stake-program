use pinocchio::program_error::ProgramError;

pub mod merge;
pub mod serialization;

pub use merge::*;
pub use serialization::*;

pub(crate) fn checked_add(a: u64, b: u64) -> Result<u64, ProgramError> {
    a.checked_add(b).ok_or(ProgramError::InsufficientFunds)
}