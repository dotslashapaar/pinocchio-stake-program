use crate::error::StakeError;
use crate::helpers::{
    bytes_to_u64, warmup_cooldown_rate, Epoch, DEFAULT_WARMUP_COOLDOWN_RATE,
};
use crate::state::stake_history::{StakeHistoryEntry, StakeHistoryGetEntry, StakeHistorySysvar};
use pinocchio::pubkey::Pubkey;

pub type StakeActivationStatus = StakeHistoryEntry;

#[repr(C, packed)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Delegation {
    /// to whom the stake is delegated
    pub voter_pubkey: Pubkey,
    /// activated stake amount, set at delegate() time
    pub stake: [u8; 8],
    /// epoch at which this stake was activated, `u64::MAX` if bootstrap stake
    pub activation_epoch: Epoch,
    /// epoch the stake was deactivated, `u64::MAX` if not deactivated
    pub deactivation_epoch: Epoch,
    /// kept for layout compatibility only; not used by logic
    #[deprecated(
        since = "1.16.7",
        note = "Use global warmup_cooldown_rate() instead"
    )]
    pub warmup_cooldown_rate: [u8; 8],
}

#[repr(C)]
#[derive(Debug, Default, PartialEq, Clone, Copy)]
pub struct Stake {
    pub delegation: Delegation,
    /// credits observed is credits from vote account state when delegated or redeemed
    pub credits_observed: [u8; 8],
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

    #[inline]
    pub fn is_bootstrap(&self) -> bool {
        bytes_to_u64(self.activation_epoch) == u64::MAX
    }

    pub fn stake<T: StakeHistoryGetEntry>(
        &self,
        epoch: Epoch,
        history: &T,
        new_rate_activation_epoch: Option<Epoch>,
    ) -> u64 {
        self.stake_activating_and_deactivating(epoch, history, new_rate_activation_epoch).effective_u64()
    }

    #[allow(clippy::comparison_chain)]
    pub fn stake_activating_and_deactivating<T: StakeHistoryGetEntry>(
        &self,
        target_epoch: Epoch,
        history: &T,
        new_rate_activation_epoch: Option<Epoch>,
    ) -> StakeActivationStatus {
        // Convert epochs to numeric before any comparisons
        let tgt = bytes_to_u64(target_epoch);
        let deact = bytes_to_u64(self.deactivation_epoch);

        // first, calculate an effective and activating stake
        let (effective_stake, activating_stake) =
            self.stake_and_activating(target_epoch, history, new_rate_activation_epoch);

        // then de-activate some portion if necessary
        if tgt < deact {
            // not deactivated
            if activating_stake == 0 {
                StakeActivationStatus::with_effective(effective_stake)
            } else {
                StakeActivationStatus::with_effective_and_activating(effective_stake, activating_stake)
            }
        } else if tgt == deact {
            // can only deactivate what's activated
            StakeActivationStatus::with_deactivating(effective_stake)
        } else if let Some((history, mut prev_epoch, mut prev_cluster_stake)) = history
            .get_entry(bytes_to_u64(self.deactivation_epoch))
            .map(|cluster_stake_at_deactivation_epoch| {
                (history, self.deactivation_epoch, cluster_stake_at_deactivation_epoch)
            })
        {
            // target_epoch > self.deactivation_epoch
            let mut current_effective_stake = effective_stake;
            loop {
                let current_epoch_u64 = bytes_to_u64(prev_epoch) + 1;

                // if there is no deactivating stake at prev epoch, we should have been fully undelegated
                if bytes_to_u64(prev_cluster_stake.deactivating) == 0 {
                    break;
                }

                // proportion of newly non-effective cluster stake this account is entitled to take
                let weight = current_effective_stake as f64
                    / bytes_to_u64(prev_cluster_stake.deactivating) as f64;
                let rate = warmup_cooldown_rate(
                    current_epoch_u64.to_le_bytes(),
                    new_rate_activation_epoch,
                );

                // newly not-effective cluster stake at current epoch
                let newly_not_effective_cluster_stake =
                    bytes_to_u64(prev_cluster_stake.effective) as f64 * rate;
                let newly_not_effective_stake =
                    ((weight * newly_not_effective_cluster_stake) as u64).max(1);

                current_effective_stake = current_effective_stake.saturating_sub(newly_not_effective_stake);
                if current_effective_stake == 0 {
                    break;
                }

                if current_epoch_u64 >= tgt {
                    break;
                }
                if let Some(current_cluster_stake) = history.get_entry(current_epoch_u64) {
                    prev_epoch = current_epoch_u64.to_le_bytes();
                    prev_cluster_stake = current_cluster_stake;
                } else {
                    break;
                }
            }

            // deactivating stake equals all of currently remaining effective stake
            StakeActivationStatus::with_deactivating(current_effective_stake)
        } else {
            // no history or dropped out of history => fully deactivated
            StakeActivationStatus::default()
        }
    }

    // returns (effective, activating)
    fn stake_and_activating<T: StakeHistoryGetEntry>(
        &self,
        target_epoch: Epoch,
        history: &T,
        new_rate_activation_epoch: Option<Epoch>,
    ) -> (u64, u64) {
        let delegated_stake = self.stake;

        let tgt = bytes_to_u64(target_epoch);
        let act = bytes_to_u64(self.activation_epoch);
        let deact = bytes_to_u64(self.deactivation_epoch);

        if self.is_bootstrap() {
            (bytes_to_u64(delegated_stake), 0)
        } else if self.activation_epoch == self.deactivation_epoch {
            (0, 0)
        } else if tgt == act {
            (0, bytes_to_u64(delegated_stake))
        } else if tgt < act {
            (0, 0)
        } else if let Some((history, mut prev_epoch, mut prev_cluster_stake)) = history
            .get_entry(bytes_to_u64(self.activation_epoch))
            .map(|cluster_stake_at_activation_epoch| {
                (history, self.activation_epoch, cluster_stake_at_activation_epoch)
            })
        {
            // tgt > act
            let mut current_effective_stake = 0u64;
            loop {
                let current_epoch_u64 = bytes_to_u64(prev_epoch) + 1;

                if bytes_to_u64(prev_cluster_stake.activating) == 0 {
                    break;
                }

                // entitlement to newly-effective cluster stake at current epoch
                let delegated_stake_u64 = bytes_to_u64(delegated_stake);
                let remaining_activating_stake = delegated_stake_u64 - current_effective_stake;
                let weight = remaining_activating_stake as f64
                    / bytes_to_u64(prev_cluster_stake.activating) as f64;
                let rate = warmup_cooldown_rate(
                    current_epoch_u64.to_le_bytes(),
                    new_rate_activation_epoch,
                );

                let newly_effective_cluster_stake =
                    bytes_to_u64(prev_cluster_stake.effective) as f64 * rate;
                let newly_effective_stake =
                    ((weight * newly_effective_cluster_stake) as u64).max(1);

                current_effective_stake = current_effective_stake.saturating_add(newly_effective_stake);
                if current_effective_stake >= delegated_stake_u64 {
                    current_effective_stake = delegated_stake_u64;
                    break;
                }

                if current_epoch_u64 >= tgt || current_epoch_u64 >= deact {
                    break;
                }
                if let Some(current_cluster_stake) = history.get_entry(current_epoch_u64) {
                    prev_epoch = current_epoch_u64.to_le_bytes();
                    prev_cluster_stake = current_cluster_stake;
                } else {
                    break;
                }
            }

            (current_effective_stake, bytes_to_u64(delegated_stake) - current_effective_stake)
        } else {
            (bytes_to_u64(delegated_stake), 0)
        }
    }
}

impl Default for Delegation {
    fn default() -> Self {
        #[allow(deprecated)]
        Self {
            voter_pubkey: Pubkey::default(),
            stake: 0u64.to_le_bytes(),
            activation_epoch: 0u64.to_le_bytes(),
            deactivation_epoch: u64::MAX.to_le_bytes(),
            warmup_cooldown_rate: DEFAULT_WARMUP_COOLDOWN_RATE.to_le_bytes(),
        }
    }
}

impl Stake {
    /// Whether this stake is considered active for the given epoch
    /// (simple window check; the effective check is done via `Stake::stake`)
    pub fn is_active(&self, current_epoch: u64, _stake_history: &StakeHistorySysvar) -> bool {
        let act = bytes_to_u64(self.delegation.activation_epoch);
        let deact = bytes_to_u64(self.delegation.deactivation_epoch);
        act <= current_epoch && current_epoch < deact
    }

    pub fn set_credits_observed(&mut self, credits: u64) {
        self.credits_observed = credits.to_le_bytes();
    }

    pub fn stake<T: StakeHistoryGetEntry>(
        &self,
        epoch: Epoch,
        history: &T,
        new_rate_activation_epoch: Option<Epoch>,
    ) -> u64 {
        self.delegation
            .stake(epoch, history, new_rate_activation_epoch)
    }

    pub fn split(
        &mut self,
        remaining_stake_delta: u64,
        split_stake_amount: u64,
    ) -> Result<Self, StakeError> {
        let current = bytes_to_u64(self.delegation.stake);
        if remaining_stake_delta > current {
            return Err(StakeError::InsufficientStake);
        }
        self.delegation.stake = current.saturating_sub(remaining_stake_delta).to_le_bytes();
        let new = Self {
            delegation: Delegation {
                stake: split_stake_amount.to_le_bytes(),
                ..self.delegation
            },
            ..*self
        };
        Ok(new)
    }

    pub fn deactivate(&mut self, epoch: Epoch) -> Result<(), StakeError> {
        if bytes_to_u64(self.delegation.deactivation_epoch) != u64::MAX {
            Err(StakeError::AlreadyDeactivated)
        } else {
            self.delegation.deactivation_epoch = epoch;
            Ok(())
        }
    }
}

// small helper to keep public API consistent
impl StakeActivationStatus {
    #[inline]
    fn effective_u64(&self) -> u64 {
        // Expect StakeHistoryEntry to expose `effective` as [u8;8] in Pinocchio
        bytes_to_u64(self.effective)
    }
}

// helper: set stake amount
impl Delegation {
    pub fn set_stake_amount(&mut self, amount: u64) {
        self.stake = amount.to_le_bytes();
    }
}
