use pinocchio::program_error::ProgramError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StakeError {
    NoCreditsToRedeem,
    LockupInForce,
    AlreadyDeactivated,
    TooSoonToRedelegate,
    InsufficientStake,
    MergeTransientStake,
    MergeMismatch,
    CustodianMissing,
    CustodianSignatureMissing,
    InsufficientDelegation,
    VoteAddressMismatch,
    MinimumDelinquentEpochsForDeactivationNotMet,
    InsufficientReferenceVotes,
    VoteAccountMustBeRentExempt,
    RedelegateTransientOrInactiveStake,
    RedelegateToSameVoteAccount,
    HistoryPoolStillInUse,
    DelayedActivationNotSupported,
    BiggerDelayedActivationRateNotSupported,
    EpochRewardsMiniPoolNotFound,
    ExpectedHistoryPoolNotFound,
}

impl From<StakeError> for ProgramError {
    fn from(e: StakeError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
