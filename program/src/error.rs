use pinocchio::program_error::ProgramError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum StakeError {
    InsufficientReferenceVotes = 6000,
    MinimumDelinquentEpochsForDeactivationNotMet,
    VoteAddressMismatch,
}

impl From<StakeError> for ProgramError {
    fn from(e: StakeError) -> Self {
        ProgramError::Custom(e as u32)
    }
}