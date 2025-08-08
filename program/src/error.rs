use pinocchio::program_error::ProgramError;

// simple internal error enum
#[derive(Debug)]
pub enum StakeError {
    InvalidAuthorization,
    InsufficientFunds,
    InsufficientStake,
    AlreadyDeactivated,
}

// map internal errors to standard program error
pub fn to_program_error(err: StakeError) -> ProgramError {
    match err {
        StakeError::InvalidAuthorization => ProgramError::MissingRequiredSignature,
        StakeError::InsufficientFunds => ProgramError::InsufficientFunds,
        StakeError::InsufficientStake => ProgramError::Custom(0x10),
        StakeError::AlreadyDeactivated => ProgramError::Custom(0x11),
    }
}
