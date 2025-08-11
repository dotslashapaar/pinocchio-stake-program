
use crate::state::{Meta, Stake, StakeFlags};
use pinocchio::program_error::ProgramError;

// This is the main enum that represents all possible states a stake account can be in
#[derive(Debug, Clone, PartialEq)]
pub enum StakeStateV2 {
    Uninitialized,                      // Account exists but nothing in it yet
    Initialized(Meta),                   // Has metadata but no stake delegation
    Stake(Meta, Stake, StakeFlags),      // Fully active stake account with delegation
    RewardsPool,                         // Special pool for rewards (rarely used)
}

impl StakeStateV2 {
    // I'm defining constants for the tags so I don't use magic numbers everywhere
    // Each variant gets a unique number to identify it when serialized
    pub const TAG_UNINITIALIZED: u8 = 0;
    pub const TAG_INITIALIZED:   u8 = 1;
    pub const TAG_STAKE:         u8 = 2;
    pub const TAG_REWARDS_POOL:  u8 = 3;

    // Calculate how many bytes we need to store the biggest variant (Stake)
    // 1 byte for the tag + Meta size + Stake size + 1 byte for flags
    pub const ACCOUNT_SIZE: usize =
        1 + core::mem::size_of::<Meta>() + core::mem::size_of::<Stake>() + 1;

    #[inline]
    pub const fn size_of() -> usize {
        // Just returning our constant size
        Self::ACCOUNT_SIZE
    }

    // Helper function to calculate where each field starts in the byte array
    #[inline]
    fn offs() -> (usize, usize, usize) {
        let meta_off  = 1;                                       // Meta starts after the tag byte
        let stake_off = meta_off + core::mem::size_of::<Meta>(); // Stake starts after Meta
        let flags_off = stake_off + core::mem::size_of::<Stake>(); // Flags start after Stake
        (meta_off, stake_off, flags_off)
    }

    // Convert raw bytes back into our enum
    pub fn deserialize(data: &[u8]) -> Result<Self, ProgramError> {
        // Need at least 1 byte for the tag
        if data.len() < 1 {
            return Err(ProgramError::InvalidAccountData);
        }
        
        // Check the first byte to see which variant we have
        match data[0] {
            Self::TAG_UNINITIALIZED => Ok(Self::Uninitialized), // Easy one, no data

            Self::TAG_INITIALIZED => {
                // Need enough bytes for Meta after the tag
                let meta_size = core::mem::size_of::<Meta>();
                if data.len() < 1 + meta_size {
                    return Err(ProgramError::InvalidAccountData);
                }
                // UNSAFE: Reading bytes directly as a Meta struct
                // read_unaligned handles cases where data isn't aligned in memory
                let meta = unsafe {
                    core::ptr::read_unaligned(data[1..1 + meta_size].as_ptr() as *const Meta)
                };
                Ok(Self::Initialized(meta))
            }

            Self::TAG_STAKE => {
                // This is the complex one - need to read Meta, Stake, and flags
                let (meta_off, stake_off, flags_off) = Self::offs();
                
                // Make sure we have enough bytes for everything
                if data.len() < flags_off + 1 {
                    return Err(ProgramError::InvalidAccountData);
                }

                // UNSAFE: Read Meta from its position
                let meta = unsafe {
                    core::ptr::read_unaligned(
                        data[meta_off..meta_off + core::mem::size_of::<Meta>()].as_ptr()
                            as *const Meta,
                    )
                };
                
                // UNSAFE: Read Stake from its position
                let stake = unsafe {
                    core::ptr::read_unaligned(
                        data[stake_off..stake_off + core::mem::size_of::<Stake>()].as_ptr()
                            as *const Stake,
                    )
                };
                
                // Read the flags byte (simple, no unsafe needed)
                let bits = data[flags_off];
                let flags = if bits == 0 { StakeFlags::empty() } else { StakeFlags { bits } };
                
                Ok(Self::Stake(meta, stake, flags))
            }

            Self::TAG_REWARDS_POOL => Ok(Self::RewardsPool), // Another easy one

            _ => Err(ProgramError::InvalidAccountData), // Unknown tag = bad data
        }
    }

    // Convert our enum into raw bytes for storage
    pub fn serialize(&self, data: &mut [u8]) -> Result<(), ProgramError> {
        // Make sure the buffer is big enough
        if data.len() < Self::ACCOUNT_SIZE {
            return Err(ProgramError::AccountDataTooSmall);
        }
        
        // Clear all bytes first for consistency (important for deterministic hashing!)
        for b in data.iter_mut() { *b = 0; }

        // Now write the appropriate data based on which variant we have
        match self {
            Self::Uninitialized => {
                data[0] = Self::TAG_UNINITIALIZED; // Just write the tag
            }
            Self::Initialized(meta) => {
                data[0] = Self::TAG_INITIALIZED; // Write tag
                // UNSAFE: Write Meta struct directly to bytes
                let dst = &mut data[1..1 + core::mem::size_of::<Meta>()];
                unsafe { core::ptr::write_unaligned(dst.as_mut_ptr() as *mut Meta, *meta); }
            }
            Self::Stake(meta, stake, flags) => {
                data[0] = Self::TAG_STAKE; // Write tag
                let (meta_off, stake_off, flags_off) = Self::offs();
                
                // UNSAFE: Write Meta and Stake structs to their positions
                unsafe {
                    // Write Meta
                    core::ptr::write_unaligned(
                        data[meta_off..meta_off + core::mem::size_of::<Meta>()].as_mut_ptr()
                            as *mut Meta,
                        *meta,
                    );
                    // Write Stake
                    core::ptr::write_unaligned(
                        data[stake_off..stake_off + core::mem::size_of::<Stake>()].as_mut_ptr()
                            as *mut Stake,
                        *stake,
                    );
                }
                // Write flags byte (usually 0 in our implementation)
                data[flags_off] = flags.bits;
            }
            Self::RewardsPool => {
                data[0] = Self::TAG_REWARDS_POOL; // Just the tag
            }
        }
        Ok(())
    }

    // Convenient helper to load from an account
    pub fn load_from_account_info(
        ai: &pinocchio::account_info::AccountInfo,
    ) -> Result<Self, ProgramError> {
        // Borrow the account data and deserialize it
        let data = ai.try_borrow_data()?;
        Self::deserialize(&data)
    }

    // Convenient helper to save back to an account
    pub fn store_to_account_info(
        &self,
        ai: &pinocchio::account_info::AccountInfo,
    ) -> Result<(), ProgramError> {
        // Get mutable access to account data and serialize into it
        let mut data = ai.try_borrow_mut_data()?;
        self.serialize(&mut data)
    }
}

// Tests to make sure everything works!
#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{
        accounts::{Authorized, Delegation, Stake},
        state::{Lockup, Meta},
    };
    use pinocchio::pubkey::Pubkey;

    // Helper to create a test Meta
    fn sample_meta() -> Meta {
        Meta {
            rent_exempt_reserve: 1234u64.to_le_bytes(), // Convert number to bytes
            authorized: Authorized {
                staker: Pubkey::default(),      // Using default (all zeros) for testing
                withdrawer: Pubkey::default(),
            },
            lockup: Lockup::default(), // Using default lockup
        }
    }

    // Helper to create a test Stake
    fn sample_stake() -> Stake {
        Stake {
            delegation: Delegation {
                voter_pubkey: Pubkey::default(),
                stake: 10_000,                  // 10k lamports staked
                activation_epoch: 7,             // Activated in epoch 7
                deactivation_epoch: u64::MAX,   // MAX means still active
                warmup_cooldown_rate: 0.25,     // 25% warmup/cooldown rate
            },
            credits_observed: 42,                // Some credits for testing
        }
    }

    #[test]
    fn roundtrip_uninitialized() {
        // Test that we can serialize and deserialize Uninitialized
        let s = StakeStateV2::Uninitialized;
        let mut buf = vec![0u8; StakeStateV2::ACCOUNT_SIZE];
        s.serialize(&mut buf).unwrap();
        
        // Check the tag is correct
        assert_eq!(buf[0], StakeStateV2::TAG_UNINITIALIZED);

        // Make sure we get the same thing back
        let back = StakeStateV2::deserialize(&buf).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn roundtrip_initialized() {
        // Test Initialized variant with Meta
        let meta = sample_meta();
        let s = StakeStateV2::Initialized(meta);
        let mut buf = vec![0u8; StakeStateV2::ACCOUNT_SIZE];
        s.serialize(&mut buf).unwrap();
        
        // Check tag
        assert_eq!(buf[0], StakeStateV2::TAG_INITIALIZED);

        // Should deserialize to same thing
        let back = StakeStateV2::deserialize(&buf).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn roundtrip_stake() {
        // Test the complex Stake variant with all fields
        let meta = sample_meta();
        let stake = sample_stake();
        // Testing with non-zero flags to make sure they're preserved
        let flags = StakeFlags { bits: 0b0000_0101 };

        let s = StakeStateV2::Stake(meta, stake, flags);
        let mut buf = vec![0u8; StakeStateV2::ACCOUNT_SIZE];
        s.serialize(&mut buf).unwrap();
        
        // Check tag
        assert_eq!(buf[0], StakeStateV2::TAG_STAKE);

        // Should get everything back correctly
        let back = StakeStateV2::deserialize(&buf).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn roundtrip_rewards_pool() {
        // Test RewardsPool variant
        let s = StakeStateV2::RewardsPool;
        let mut buf = vec![0u8; StakeStateV2::ACCOUNT_SIZE];
        s.serialize(&mut buf).unwrap();
        assert_eq!(buf[0], StakeStateV2::TAG_REWARDS_POOL);

        let back = StakeStateV2::deserialize(&buf).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn serialize_too_small_buffer_fails() {
        // Make sure we get error if buffer is too small
        let s = StakeStateV2::Uninitialized;
        let mut tiny = vec![0u8; 0]; // Way too small!
        let err = s.serialize(&mut tiny).unwrap_err();
        assert_eq!(err, ProgramError::AccountDataTooSmall);
    }

    #[test]
    fn deserialize_invalid_tag_fails() {
        // Test that invalid tags cause errors
        let mut buf = vec![0u8; StakeStateV2::ACCOUNT_SIZE];
        buf[0] = 255; // This isn't a valid tag!
        let err = StakeStateV2::deserialize(&buf).unwrap_err();
        assert_eq!(err, ProgramError::InvalidAccountData);
    }

    #[test]
    fn account_size_is_large_enough() {
        // Verify our size calculation is correct
        let want = 1 + core::mem::size_of::<Meta>() + core::mem::size_of::<Stake>() + 1;
        assert_eq!(StakeStateV2::ACCOUNT_SIZE, want);
    }
}