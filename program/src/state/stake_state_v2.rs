use crate::state::accounts::{Meta, Stake};
use crate::state::stake_flag::StakeFlags;
pub enum StakeStateV2 {
    Uninitialized,
    Initialized(Meta),
    Stake(Meta, Stake, StakeFlags),
    RewardsPool,
}

impl StakeStateV2 {
    /// The fixed number of bytes used to serialize each stake account
    pub const fn size_of() -> usize {
        200 // see test_size_of
    }
}
