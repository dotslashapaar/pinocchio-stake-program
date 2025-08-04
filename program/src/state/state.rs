use crate::helpers::DEFAULT_WARMUP_COOLDOWN_RATE;
use crate::state::accounts::Authorized;
use pinocchio::{
    pubkey::Pubkey,
    sysvars::clock::{Epoch, UnixTimestamp},
};
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct Lockup {
    /// UnixTimestamp at which this stake will allow withdrawal, unless the
    ///   transaction is signed by the custodian
    pub unix_timestamp: UnixTimestamp,
    /// epoch height at which this stake will allow withdrawal, unless the
    ///   transaction is signed by the custodian
    pub epoch: Epoch,
    /// custodian signature on a transaction exempts the operation from
    ///  lockup constraints
    pub custodian: Pubkey,
}

#[repr(C)]
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct Meta {
    pub rent_exempt_reserve: [u8; 8], // Using array for fixed size
    pub authorized: Authorized,
    pub lockup: Lockup,
}

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
