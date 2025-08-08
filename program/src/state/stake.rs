use crate::{
    error::*, helpers::*, state::delegation::Delegation, state::stake_history::StakeHistoryGetEntry,
};

#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct Stake {
    /// Delegation information
    pub delegation: Delegation,
    /// Credits observed during the epoch
    pub credits_observed: u64,
}

impl Stake {
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
        if remaining_stake_delta > bytes_to_u64(self.delegation.stake) {
            return Err(StakeError::InsufficientStake);
        }
        let updated_stake =
            bytes_to_u64(self.delegation.stake).saturating_sub(remaining_stake_delta);
        self.delegation.stake = updated_stake.to_le_bytes();
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
