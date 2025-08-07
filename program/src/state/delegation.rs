use crate::helpers::*;
use crate::state::stake_history::{StakeHistoryEntry, StakeHistoryGetEntry};

use pinocchio::pubkey::Pubkey;

pub type StakeActivationStatus = StakeHistoryEntry;

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
    pub fn is_bootstrap(&self) -> bool {
        bytes_to_u64(self.activation_epoch) == u64::MAX
    }
    // pub fn stake_activating_and_deactivating<T: StakeHistoryGetEntry>(
    //     &self,
    //     target_epoch: Epoch,
    //     history: &T,
    //     new_rate_activation_epoch: Option<Epoch>,
    // ) -> StakeActivationStatus {
    //     // first, calculate an effective and activating stake
    //     let (effective_stake, activating_stake) =
    //         self.stake_and_activating(target_epoch, history, new_rate_activation_epoch);
    // }

    fn stake_and_activating<T: StakeHistoryGetEntry>(
        &self,
        target_epoch: Epoch,
        history: &T,
        new_rate_activation_epoch: Option<Epoch>,
    ) -> (u64, u64) {
        let delegated_stake = self.stake;
        if self.is_bootstrap() {
            // fully effective immediately
            (bytes_to_u64(delegated_stake), 0)
        } else if self.activation_epoch == self.deactivation_epoch {
            // activated but instantly deactivated; no stake at all regardless of target_epoch
            // this must be after the bootstrap check and before all-is-activating check
            (0, 0)
        } else if target_epoch == self.activation_epoch {
            // all is activating
            (0, bytes_to_u64(delegated_stake))
        } else if target_epoch < self.activation_epoch {
            // not yet enabled
            (0, 0)
        } else if let Some((history, mut prev_epoch, mut prev_cluster_stake)) = history
            .get_entry(bytes_to_u64(self.activation_epoch))
            .map(|cluster_stake_at_activation_epoch| {
                (
                    history,
                    self.activation_epoch,
                    cluster_stake_at_activation_epoch,
                )
            })
        {
            // target_epoch > self.activation_epoch

            // loop from my activation epoch until the target epoch summing up my entitlement
            // current effective stake is updated using its previous epoch's cluster stake
            let mut current_epoch;
            let mut current_effective_stake = 0;
            loop {
                current_epoch = bytes_to_u64(prev_epoch) + 1;
                // if there is no activating stake at prev epoch, we should have been
                // fully effective at this moment
                if bytes_to_u64(prev_cluster_stake.activating) == 0 {
                    break;
                }
                // let remaining_activating_stake = delegated_stake - current_effective_stake;
                // let weight =
                //     remaining_activating_stake as f64 / prev_cluster_stake.activating as f64;
                // let warmup_cooldown_rate =
                //     warmup_cooldown_rate(current_epoch, new_rate_activation_epoch);

                // how much of the growth in stake this account is
                //  entitled to take
                let delegated_stake_u64 = bytes_to_u64(delegated_stake);
                let prev_cluster_activating_u64 = bytes_to_u64(prev_cluster_stake.activating);
                let remaining_activating_stake = delegated_stake_u64 - current_effective_stake;
                let weight = remaining_activating_stake as f64 / prev_cluster_activating_u64 as f64;
                let warmup_cooldown_rate =
                    warmup_cooldown_rate(current_epoch.to_le_bytes(), new_rate_activation_epoch);

                // portion of newly effective cluster stake I'm entitled to at current epoch
                let prev_cluster_stake_effective = bytes_to_u64(prev_cluster_stake.effective);
                let newly_effective_cluster_stake =
                    prev_cluster_stake_effective as f64 * warmup_cooldown_rate;
                let newly_effective_stake =
                    ((weight * newly_effective_cluster_stake) as u64).max(1);

                current_effective_stake =
                    current_effective_stake.saturating_add(newly_effective_stake);
                if current_effective_stake >= bytes_to_u64(delegated_stake) {
                    current_effective_stake = bytes_to_u64(delegated_stake);
                    break;
                }

                if current_epoch >= bytes_to_u64(target_epoch)
                    || current_epoch >= bytes_to_u64(self.deactivation_epoch)
                {
                    break;
                }
                if let Some(current_cluster_stake) = history.get_entry(current_epoch) {
                    prev_epoch = current_epoch.to_le_bytes();
                    prev_cluster_stake = (current_cluster_stake);
                } else {
                    break;
                }
            }

            (
                current_effective_stake,
                bytes_to_u64(delegated_stake) - current_effective_stake,
            )
        } else {
            // no history or I've dropped out of history, so assume fully effective
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
