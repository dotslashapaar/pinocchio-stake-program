#[repr(C)]
#[derive(Copy, PartialEq, Eq, Clone, PartialOrd, Ord, Hash, Debug)]
pub struct StakeFlags {
    bits: u8,
}

/// Currently, only bit 1 is used. The other 7 bits are reserved for future usage.
impl StakeFlags {
    ///  Stake must be fully activated before deactivation is allowed (bit 1).
    #[deprecated(
        since = "2.1.0",
        note = "This flag will be removed because it was only used for `redelegate`, which will not be enabled."
    )]
    pub const MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED: Self =
        Self { bits: 0b0000_0001 };

    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    pub const fn contains(&self, other: Self) -> bool {
        (self.bits & other.bits) == other.bits
    }

    pub fn remove(&mut self, other: Self) {
        self.bits &= !other.bits;
    }

    pub fn set(&mut self, other: Self) {
        self.bits |= other.bits;
    }

    pub const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }
}

impl Default for StakeFlags {
    fn default() -> Self {
        StakeFlags::empty()
    }
}
