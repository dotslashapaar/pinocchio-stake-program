use pinocchio::{program_error::ProgramError, pubkey::Pubkey, sysvars::clock::Clock};

use crate::error::StakeError;

/// stake history sysvar placeholder
#[derive(Clone, Debug)]
pub struct StakeHistory {
    pub epoch: u64,
}

impl StakeHistory {
    pub fn new(epoch: u64) -> Self {
        Self { epoch }
    }
}

/// Core stake account metadata
#[derive(Clone, Debug, PartialEq)]
pub struct Meta {
    pub rent_exempt_reserve: u64,
    pub authorized: Authorized,
    pub lockup: Lockup,
}

impl Meta {
    pub fn new(rent_exempt_reserve: u64, authorized: Authorized, lockup: Lockup) -> Self {
        Self {
            rent_exempt_reserve,
            authorized,
            lockup,
        }
    }

    pub fn size() -> usize {
        core::mem::size_of::<Meta>()
    }
}

/// stake authorization keys
#[derive(Clone, Debug, PartialEq)]
#[repr(C)]
pub struct Authorized {
    pub staker: Pubkey,
    pub withdrawer: Pubkey,
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
}

/// stake lockup restrictions  
#[derive(Clone, Debug, PartialEq)]
pub struct Lockup {
    pub unix_timestamp: i64,
    pub epoch: u64,
    pub custodian: Pubkey,
}

impl Lockup {
    pub fn default() -> Self {
        Self {
            unix_timestamp: 0,
            epoch: 0,
            custodian: Pubkey::default(),
        }
    }

    pub fn new(unix_timestamp: i64, epoch: u64, custodian: Pubkey) -> Self {
        Self {
            unix_timestamp,
            epoch,
            custodian,
        }
    }
}


/// delegation info for active stakes
#[derive(Clone, Debug, PartialEq)]
pub struct Delegation {
    pub voter_pubkey: Pubkey,
    pub stake: u64,
    pub activation_epoch: u64,
    pub deactivation_epoch: u64,
    pub warmup_cooldown_rate: f64,
}

impl Delegation {
    pub fn new(voter_pubkey: Pubkey, stake: u64, activation_epoch: u64) -> Self {
        Self {
            voter_pubkey,
            stake,
            activation_epoch,
            deactivation_epoch: u64::MAX,
            warmup_cooldown_rate: 0.25,
        }
    }
}

/// Active stake data
#[derive(Clone, Debug, PartialEq)]
pub struct Stake {
    pub delegation: Delegation,
    pub credits_observed: u64,
}

impl Stake {
    pub fn new(delegation: Delegation, credits_observed: u64) -> Self {
        Self {
            delegation,
            credits_observed,
        }
    }
}

/// all possible stake account states
#[derive(Clone, Debug, PartialEq)]
pub enum StakeStateV2 {
    Uninitialized,
    Initialized(Meta),
    Stake(Meta, Stake, StakeFlags),
    RewardsPool,
}

/// stake account flags
#[derive(Clone, Debug, PartialEq)]
pub struct StakeFlags {
    pub bits: u8,
}

impl StakeFlags {
    pub fn empty() -> Self {
        Self { bits: 0 }
    }
}

impl StakeStateV2 {
    /// Account size for stake state
    pub fn size_of() -> usize {
        200 
    }
}

/// Types of mergeable stake states
#[derive(Clone, Debug, PartialEq)]
pub enum MergeKind {
    Inactive(Meta, u64, StakeFlags),
    ActivationEpoch(Meta, Stake, StakeFlags),
    FullyActive(Meta, Stake),
}

impl MergeKind {
    /// Get metadata from any merge kind
    pub fn meta(&self) -> &Meta {
        match self {
            MergeKind::Inactive(meta, _, _) => meta,
            MergeKind::ActivationEpoch(meta, _, _) => meta,
            MergeKind::FullyActive(meta, _) => meta,
        }
    }

    /// Check if stake state can be merged
    pub fn get_if_mergeable(
        stake_state: &StakeStateV2,
        lamports: u64,
        _clock: &Clock,
        _stake_history: &StakeHistory,
    ) -> Result<Self, ProgramError> {
        match stake_state {
            StakeStateV2::Stake(meta, stake, stake_flags) => {
                // Simplified logic, later => this would check epochs
                if stake.delegation.deactivation_epoch == u64::MAX {
                    Ok(MergeKind::FullyActive(meta.clone(), stake.clone()))
                } else {
                    Ok(MergeKind::ActivationEpoch(meta.clone(), stake.clone(), stake_flags.clone()))
                }
            }
            StakeStateV2::Initialized(meta) => {
                Ok(MergeKind::Inactive(meta.clone(), lamports, StakeFlags::empty()))
            }
            _ => Err(ProgramError::InvalidAccountData),
        }
    }

    /// Verify metas can be merged (authorities and lockups match)
    pub fn metas_can_merge(
        stake: &Meta,
        source: &Meta,
        clock: &Clock,
    ) -> Result<(), ProgramError> {
        // Check authorities match
        if stake.authorized.staker != source.authorized.staker {
            return Err(StakeError::MergeMismatch.into());
        }

        if stake.authorized.withdrawer != source.authorized.withdrawer {
            return Err(StakeError::MergeMismatch.into());
        }

        // Check lockups if active
        if stake.lockup.unix_timestamp > clock.unix_timestamp
            || stake.lockup.epoch > clock.epoch
        {
            if stake.lockup.unix_timestamp != source.lockup.unix_timestamp
                || stake.lockup.epoch != source.lockup.epoch
            {
                return Err(StakeError::LockupInForce.into());
            }
        }

        Ok(())
    }
}