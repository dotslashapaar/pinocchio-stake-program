use crate::helpers::DEFAULT_WARMUP_COOLDOWN_RATE;
use crate::helpers::utils::StakeHistorySysvar;

use pinocchio::sysvars::clock::{Epoch, UnixTimestamp};
use pinocchio::{account_info::AccountInfo, pubkey::Pubkey};
#[repr(C)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Delegation {
    /// to whom the stake is delegated
    pub voter_pubkey: Pubkey,
    /// activated stake amount, set at delegate() time
    pub stake: [u8; 8],
    /// epoch at which this stake was activated, `std::u64::MAX` if is a bootstrap stake
    pub activation_epoch: Epoch,
    /// epoch the stake was deactivated, `std::u64::MAX` if not deactivated
    pub deactivation_epoch: Epoch,
    /// how much stake we can activate per-epoch as a fraction of currently effective stake
    #[deprecated(
        since = "1.16.7",
        note = "Please use `solana_sdk::stake::state::warmup_cooldown_rate()` instead"
    )]
    pub warmup_cooldown_rate: [u8; 8],
}

#[repr(C)]
#[derive(Debug, Default, PartialEq, Clone, Copy)]
pub struct Stake {
    pub delegation: Delegation,
    /// credits observed is credits from vote account state when delegated or redeemed
    pub credits_observed: [u8; 8],
    // changed to pub (as required in utils.rs L511 and L455)
}

impl Delegation {
    pub fn new(voter_pubkey: &Pubkey, stake: u64, activation_epoch: Epoch) -> Self {
        Self {
            voter_pubkey: *voter_pubkey,
            stake: stake.to_le_bytes(),
            activation_epoch,
            ..Delegation::default()
        }
    }
}

impl Default for Delegation {
    fn default() -> Self {
        #[allow(deprecated)]
        Self {
            voter_pubkey: Pubkey::default(),
            stake: 0u64.to_le_bytes(),
            activation_epoch: 0u64,
            deactivation_epoch: u64::MAX,
            warmup_cooldown_rate: DEFAULT_WARMUP_COOLDOWN_RATE.to_le_bytes(),
        }
    }
}

// helper: check if stake is active for current epoch
impl Stake {
    pub fn is_active(&self, current_epoch: u64, _stake_history: &StakeHistorySysvar) -> bool {
        self.delegation.activation_epoch <= current_epoch
            && current_epoch < self.delegation.deactivation_epoch
    }

    pub fn set_credits_observed(&mut self, credits: u64) {
        self.credits_observed = credits.to_le_bytes();
    }
}

// helper: set stake amount
impl Delegation {
    pub fn set_stake_amount(&mut self, amount: u64) {
        self.stake = amount.to_le_bytes();
    }
}
