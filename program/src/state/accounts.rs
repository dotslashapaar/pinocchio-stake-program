use pinocchio::{
    account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey, sysvars::clock::{self, Clock, Epoch, UnixTimestamp}
};

#[repr(u8)]
pub enum StakeState {
  Uninitialized,
  Initialized(Meta),
  Stake(Meta, Stake),
  RewardPool
}

#[derive(Clone, PartialEq)]
#[repr(C)]
pub struct Meta{
    pub rent_exempt_reserve: Pubkey,
    pub authorized: Pubkey,
    pub lockup: Pubkey,
}

impl Meta {
    pub fn size() -> usize {
        core::mem::size_of::<Meta>()
    }

    pub fn get_account_info(account: &AccountInfo) -> Result<&mut Self, ProgramError> {
        if account.data_len() < core::mem::size_of::<Meta>() {
            return Err(ProgramError::InvalidAccountData);
        };

        if !account.is_writable() {
            return Err(ProgramError::InvalidAccountData);
        }

        if account.owner() != &crate::ID {
            return Err(ProgramError::IncorrectProgramId);
        };

        return Ok( unsafe { &mut *(account.borrow_data_unchecked().as_ptr() as *mut Self) });
    }
}

#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct Authorized {
    /// Authority to manage the stake account (delegate, deactivate, split, merge)
    pub staker: Pubkey,

    /// Authority to withdraw funds from the stake account
    pub withdrawer: Pubkey
}

impl Authorized {
    pub const fn size() -> usize {
        8 + core::mem::size_of::<Authorized>()
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

    pub fn get_account_info(accounts: &AccountInfo) -> &mut Self {
        unsafe { &mut *(accounts.borrow_mut_data_unchecked().as_ptr() as *mut Self) }
    }
}

#[repr(C)]
pub struct Lockup {
    /// Unix timestamp at which this stake will allow withdrawal, unless the transaction is signed by the custodian
    pub unix_timestamp: UnixTimestamp,
    /// Epoch height at which this stake will allow withdrawal, unless the transaction is signed by the custodian
    pub epoch: Epoch,
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
            custodian
        }
    }

    /// Check if the lockup is active for the given timestamp and epoch
    pub fn is_active(&self, current_timestamp: i64, current_epoch: u64) -> bool {
        current_timestamp < self.unix_timestamp || current_epoch < self.epoch
    }

    pub fn get_account_info(account: &AccountInfo) -> Result<&mut Self, ProgramError> {
        let data = account.try_borrow_mut_data().unwrap();

        if data.len() < Self::size() {
            return Err(ProgramError::InvalidAccountData);
        };

        if !account.is_writable() {
            return Err(ProgramError::InvalidAccountData);
        };

        if account.owner() != &crate::ID {
            return Err(ProgramError::IncorrectProgramId);
        };

        return Ok(
            unsafe { 
                &mut *(account.borrow_mut_data_unchecked().as_ptr() as *mut Self) 
            }
        );
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
    /// Epoch at which this delegation was deactivated, or std::u64::MAX if never deactivated
    pub deactivation_epoch: u64,
    /// How much stake we can activate per-epoch as a fraction of currently effective stake
    pub warmup_cooldown_rate: f64,
}

impl Delegation {
    pub fn size() -> usize {
        core::mem::size_of::<Delegation>()
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
#[derive(Debug, Clone, PartialEq)]
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

/// Complete stake history
#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct StakeHistory {
    /// Vector of stake history entries
    pub entries: [StakeHistoryEntry; 10],
}

/// Initialize stake account instruction data
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

/// Authorize with seed instruction data
#[repr(C)]
pub struct AuthorizeWithSeedData<'a>{
    pub new_authorized: Pubkey,
    pub stake_authorize: StakeAuthorize,
    pub authority_seed: &'a [u8],
    pub authority_owner: Pubkey,
}

impl<'a> AuthorizeWithSeedData<'a> {
    pub const fn size() -> usize {
        core::mem::size_of::<AuthorizeWithSeedData>()
    }
}

#[derive(Clone)]
pub struct SetLockupData {
    pub unix_timestamp: Option<i64>,
    pub epoch: Option<u64>,
    pub custodian: Option<Pubkey>, 
}

impl SetLockupData {
    pub fn instruction_data(data: &[u8]) -> &mut Self {
        unsafe { &mut *(data.as_ptr() as *mut Self) }
    }
}
