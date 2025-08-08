
pub mod stake_error {
    use pinocchio::program_error::ProgramError;

    #[derive(Debug, Clone, PartialEq)]
    pub enum StakeError {
        InsufficientStake,
        AlreadyDeactivated,
    }

    impl From<StakeError> for ProgramError {
        fn from(e: StakeError) -> Self {
            match e {
                StakeError::InsufficientStake => ProgramError::Custom(0x10),
                StakeError::AlreadyDeactivated => ProgramError::Custom(0x11),
            }
        }

use pinocchio::program_error::ProgramError;

// simple internal error enum
#[derive(Debug)]
pub enum StakeError {
    InvalidAuthorization,
    InsufficientFunds,
}

// map internal errors to standard program error
pub fn to_program_error(err: StakeError) -> ProgramError {
    match err {
        StakeError::InvalidAuthorization => ProgramError::MissingRequiredSignature,
        StakeError::InsufficientFunds => ProgramError::InsufficientFunds,

    }
}
