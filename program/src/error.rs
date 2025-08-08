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
    }
}
