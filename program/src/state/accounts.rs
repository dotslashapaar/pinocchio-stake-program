
use core::mem::size_of;
use pinocchio::program_error::ProgramError;
use pinocchio::pubkey::Pubkey;

use crate::state::Lockup;

/// Max seed length for `AuthorizeWithSeed` (matches Solana convention)
pub const MAX_AUTHORITY_SEED_LEN: usize = 32;

#[repr(C)]
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct Authorized {
    /// Authority to manage the stake account (delegate, deactivate, split, merge)
    pub staker: Pubkey,
    /// Authority to withdraw funds from the stake account
    pub withdrawer: Pubkey,
}

impl Authorized {
    #[inline]
    pub const fn size() -> usize {
        core::mem::size_of::<Authorized>()
    }

    #[inline]
    pub fn new(staker: Pubkey, withdrawer: Pubkey) -> Self {
        Self { staker, withdrawer }
    }

    #[inline]
    pub fn is_staker(&self, pubkey: &Pubkey) -> bool {
        self.staker == *pubkey
    }

    #[inline]
    pub fn is_withdrawer(&self, pubkey: &Pubkey) -> bool {
        self.withdrawer == *pubkey
    }

    /// Simple signature check: did the required signer appear?
    #[inline]
    pub fn check(
        &self,
        signers: &[Pubkey],
        which: StakeAuthorize,
    ) -> Result<(), ProgramError> {
        let required = match which {
            StakeAuthorize::Staker => &self.staker,
            StakeAuthorize::Withdrawer => &self.withdrawer,
        };
        if signers.iter().any(|k| k == required) {
            Ok(())
        } else {
            Err(ProgramError::MissingRequiredSignature)
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
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
    #[inline]
    pub fn size() -> usize {
        size_of::<Delegation>()
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        self.deactivation_epoch == u64::MAX
    }

    #[inline]
    pub fn is_fully_activated(&self, current_epoch: u64) -> bool {
        current_epoch >= self.activation_epoch
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stake {
    /// Delegation information
    pub delegation: Delegation,
    /// Credits observed during the epoch
    pub credits_observed: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    /// How much stake we can activate/deactivate per-epoch as a fraction of currently effective stake
    pub warmup_cooldown_rate: f64,
    /// Percentage of stake lost when slashing a stake account
    pub slash_penalty: u8,
}

impl Config {
    #[inline]
    pub const fn size() -> usize {
        core::mem::size_of::<Config>()
    }
}

/// Initialize stake account instruction data
#[repr(C)]
pub struct InitializeData {
    pub authorized: Authorized,
    pub lockup: Lockup,
}

impl InitializeData {
    #[inline]
    pub const fn size() -> usize {
        Authorized::size() + Lockup::size()
    }
}

/// Delegate stake instruction data
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DelegateStakeData {
    pub vote_pubkey: Pubkey,
}

impl DelegateStakeData {
    #[inline]
    pub const fn size() -> usize {
        core::mem::size_of::<DelegateStakeData>()
    }
}

/// Split stake instruction data
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitData {
    pub lamports: u64,
}

impl SplitData {
    #[inline]
    pub const fn size() -> usize {
        core::mem::size_of::<SplitData>()
    }
}

/// Withdraw instruction data
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WithdrawData {
    pub lamports: u64,
}

impl WithdrawData {
    #[inline]
    pub const fn size() -> usize {
        core::mem::size_of::<WithdrawData>()
    }
}

/// Authorize instruction data
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AuthorizeData {
    pub new_authorized: Pubkey,
    pub stake_authorize: StakeAuthorize,
}

impl AuthorizeData {
    #[inline]
    pub const fn size() -> usize {
        core::mem::size_of::<AuthorizeData>()
    }
}

/// Types of stake authorization (wire-encoded as a single byte)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StakeAuthorize {
    Staker = 0,
    Withdrawer = 1,
}

/// Authorize-with-seed instruction data (parsed view over raw bytes)
#[repr(C)]
pub struct AuthorizeWithSeedData<'a> {
    pub new_authorized: Pubkey,
    pub stake_authorize: StakeAuthorize,
    pub authority_seed: &'a [u8],
    pub authority_owner: Pubkey,
}

impl<'a> AuthorizeWithSeedData<'a> {
    #[inline]
    pub const fn size() -> usize {
        core::mem::size_of::<AuthorizeWithSeedData>()
    }

    /// Parse wire format:
    /// [0..32) new_authorized | [32] auth (0/1) | [33] seed_len |
    /// [34..34+seed_len) seed | [..+32) owner pubkey
    pub fn parse(data: &'a [u8]) -> Result<Self, ProgramError> {
        if data.len() < 34 + 32 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let mut new_authorized = [0u8; 32];
        new_authorized.copy_from_slice(&data[0..32]);

        let stake_authorize = match data[32] {
            0 => StakeAuthorize::Staker,
            1 => StakeAuthorize::Withdrawer,
            _ => return Err(ProgramError::InvalidInstructionData),
        };

        let seed_len = data[33] as usize;
        if seed_len > MAX_AUTHORITY_SEED_LEN {
            return Err(ProgramError::InvalidInstructionData);
        }
        if data.len() < 34 + seed_len + 32 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let authority_seed = &data[34..34 + seed_len];

        let mut authority_owner = [0u8; 32];
        authority_owner.copy_from_slice(&data[34 + seed_len..34 + seed_len + 32]);

        Ok(Self {
            new_authorized,
            stake_authorize,
            authority_seed,
            authority_owner,
        })
    }
}

/// Helper used by SetLockup ix builders (kept as-is because other code may use it)
#[derive(Clone)]
pub struct SetLockupData {
    pub unix_timestamp: Option<i64>,
    pub epoch: Option<u64>,
    pub custodian: Option<Pubkey>,
}

impl SetLockupData {
    pub const LEN: usize = 1 + 8 + 1 + 8 + 1 + 32; // flags + ts + flag + epoch + flag + pubkey

    #[inline]
    pub fn instruction_data(data: &[u8]) -> &mut Self {
        // SAFETY: the caller ensures the layout/length matches
        unsafe { &mut *(data.as_ptr() as *mut Self) }
    }
}