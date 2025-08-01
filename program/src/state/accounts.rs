use core::mem::size_of;
use pinocchio::pubkey::Pubkey;

// Constants for fixed-size arrays
pub const MAX_STAKE_HISTORY_ENTRIES: usize = 512;
pub const MAX_AUTHORITY_SEED_LEN: usize = 32;

#[repr(u8)]
pub enum StakeState {
    /// Account is not yet initialized
    Uninitialized = 0,

    /// Account is initialized with stake metadata
    Initialized = 1,

    /// Account is a delegated stake account
    Stake = 2,

    /// Account represents rewards that were distributed to stake accounts
    RewardsPool = 3,
}

#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct Meta {
    pub rent_exempt_reserve: u64,
    pub authorized: Authorized,
    pub lockup: Lockup,
}

impl Meta {
    pub fn size() -> usize {
        core::mem::size_of::<Meta>()
    }
}

#[repr(C)]
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct Authorized {
    /// Authority to manage the stake account (delegate, deactivate, split, merge)
    pub staker: Pubkey,

    /// Authority to withdraw funds from the stake account
    pub withdrawer: Pubkey,
}

impl Authorized {
    pub const fn size() -> usize {
        8 + size_of::<Authorized>()
    }

    pub fn new(staker: Pubkey, withdrawer: Pubkey) -> Self {
        Self { staker, withdrawer }
    }

    pub fn is_staker(&self, pubkey: &Pubkey) -> bool {
        self.staker == *pubkey
    }

    pub fn is_withdrawer(&self, pubkey: &Pubkey) -> bool {
        self.withdrawer == *pubkey
    }
}

#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct Lockup {
    /// Unix timestamp at which this stake will allow withdrawal, unless the transaction is signed by the custodian
    pub unix_timestamp: i64,
    /// Epoch height at which this stake will allow withdrawal, unless the transaction is signed by the custodian
    pub epoch: u64,
    // Custodian signature on a transaction exempts the operation from lockup constraints
    pub custodian: Pubkey,
}

impl Lockup {
    pub const fn size() -> usize {
        core::mem::size_of::<Lockup>()
    }

    /// Create a new lockup
    pub fn new(unix_timestamp: i64, epoch: u64, custodian: Pubkey) -> Self {
        Self {
            unix_timestamp,
            epoch,
            custodian,
        }
    }

    /// Check if the lockup is active for the given timestamp and epoch
    pub fn is_active(&self, current_timestamp: i64, current_epoch: u64) -> bool {
        current_timestamp < self.unix_timestamp || current_epoch < self.epoch
    }
}

#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct Stake {
    /// Delegation information
    pub delegation: Delegation,
    /// Credits observed during the epoch
    pub credits_observed: u64,
}

#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct Delegation {
    /// To whom the stake is delegated
    pub voter_pubkey: Pubkey,
    /// Amount of stake delegated, in lamports
    pub stake: u64,
    /// Epoch at which this delegation was activated
    pub activation_epoch: u64,
    /// Epoch at which this delegation was deactivated, or u64::MAX if never deactivated
    pub deactivation_epoch: u64,
    /// How much stake we can activate per-epoch as a fraction of currently effective stake
    pub warmup_cooldown_rate: f64,
}

impl Delegation {
    pub fn size() -> usize {
        size_of::<Delegation>()
    }

    /// Check if the delegation is active
    pub fn is_active(&self) -> bool {
        self.deactivation_epoch == u64::MAX
    }

    /// Check if the delegation is fully activated
    pub fn is_fully_activated(&self, current_epoch: u64) -> bool {
        current_epoch >= self.activation_epoch
    }
}

/// Configuration parameters for the stake program
#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct Config {
    /// How much stake we can activate/deactivate per-epoch as a fraction of currently effective stake
    pub warmup_cooldown_rate: f64,
    /// Percentage of stake lost when slashing a stake account
    pub slash_penalty: u8,
}

impl Config {
    pub const fn size() -> usize {
        core::mem::size_of::<Config>()
    }
}

/// Stake history entry
#[derive(Debug, Clone, PartialEq, Copy)]
#[repr(C)]
pub struct StakeHistoryEntry {
    /// Epoch for which this entry applies
    pub epoch: u64,
    /// Effective stake amount for this epoch
    pub effective: u64,
    /// Activating stake amount for this epoch
    pub activating: u64,
    /// Deactivating stake amount for this epoch
    pub deactivating: u64,
}

impl StakeHistoryEntry {
    pub const fn size() -> usize {
        core::mem::size_of::<StakeHistoryEntry>()
    }
}

/// Complete stake history with fixed-size array
#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct StakeHistory {
    /// Fixed-size array of stake history entries
    pub entries: [StakeHistoryEntry; MAX_STAKE_HISTORY_ENTRIES],
    /// Number of valid entries in the array
    pub len: usize,
}

impl StakeHistory {
    pub fn new() -> Self {
        Self {
            entries: [StakeHistoryEntry {
                epoch: 0,
                effective: 0,
                activating: 0,
                deactivating: 0,
            }; MAX_STAKE_HISTORY_ENTRIES],
            len: 0,
        }
    }

    pub fn push(&mut self, entry: StakeHistoryEntry) -> Result<(), &'static str> {
        if self.len >= MAX_STAKE_HISTORY_ENTRIES {
            return Err("StakeHistory is full");
        }
        self.entries[self.len] = entry;
        self.len += 1;
        Ok(())
    }

    pub fn get(&self, index: usize) -> Option<&StakeHistoryEntry> {
        if index < self.len {
            Some(&self.entries[index])
        } else {
            None
        }
    }
}

/// Initialize stake account instruction data
#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct InitializeData {
    pub authorized: Authorized,
    pub lockup: Lockup,
}

impl InitializeData {
    pub const fn size() -> usize {
        Authorized::size() + Lockup::size()
    }
}

// Delegate stake instruction data
#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct DelegateStakeData {
    pub vote_pubkey: Pubkey,
}

impl DelegateStakeData {
    pub const fn size() -> usize {
        core::mem::size_of::<DelegateStakeData>()
    }
}

// Split stake instruction data
#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct SplitData {
    pub lamports: u64,
}

impl SplitData {
    pub const fn size() -> usize {
        core::mem::size_of::<SplitData>()
    }
}

// Withdraw instruction data
#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct WithdrawData {
    pub lamports: u64,
}

impl WithdrawData {
    pub const fn size() -> usize {
        core::mem::size_of::<WithdrawData>()
    }
}

// Authorize instruction data
#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct AuthorizeData {
    pub new_authorized: Pubkey,
    pub stake_authorize: StakeAuthorize,
}

impl AuthorizeData {
    pub const fn size() -> usize {
        core::mem::size_of::<AuthorizeData>()
    }
}

/// Types of stake authorization
#[derive(Debug, Clone, PartialEq)]
#[repr(u8)]
pub enum StakeAuthorize {
    Staker = 0,
    Withdrawer = 1,
}

/// Authorize with seed instruction data using fixed-size byte array for seed
// #[derive(Debug, Clone, PartialEq)]
// #[repr(C)]
// pub struct AuthorizeWithSeedData {
//     pub new_authorized: Pubkey,
//     pub stake_authorize: StakeAuthorize,
//     /// Fixed-size byte array for authority seed
//     pub authority_seed: [u8; MAX_AUTHORITY_SEED_LEN],
//     /// Length of the actual seed data
//     pub authority_seed_len: u8,
//     pub authority_owner: Pubkey,
// }

// impl AuthorizeWithSeedData {
//     pub const fn size() -> usize {
//         core::mem::size_of::<AuthorizeWithSeedData>()
//     }

//     /// Create new instance with seed as byte slice
//     pub fn new(
//         new_authorized: Pubkey,
//         stake_authorize: StakeAuthorize,
//         authority_seed: &[u8],
//         authority_owner: Pubkey,
//     ) -> Result<Self, &'static str> {
//         if authority_seed.len() > MAX_AUTHORITY_SEED_LEN {
//             return Err("Authority seed too long");
//         }

//         let mut seed_array = [0u8; MAX_AUTHORITY_SEED_LEN];
//         seed_array[..authority_seed.len()].copy_from_slice(authority_seed);

//         Ok(Self {
//             new_authorized,
//             stake_authorize,
//             authority_seed: seed_array,
//             authority_seed_len: authority_seed.len() as u8,
//             authority_owner,
//         })
//     }

//     /// Get the authority seed as a slice
//     pub fn get_authority_seed(&self) -> &[u8] {
//         &self.authority_seed[..self.authority_seed_len as usize]
//     }
// }

/// Set lockup instruction data
#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct LockupData {
    pub unix_timestamp: Option<i64>,
    pub epoch: Option<u64>,
    pub custodian: Option<Pubkey>,
}

impl LockupData {
    pub const fn size() -> usize {
        core::mem::size_of::<LockupData>()
    }
}
