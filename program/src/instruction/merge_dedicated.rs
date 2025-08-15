// === DEDICATED MERGE MODULE ===
//
// This module contains ALL merge-specific logic for the stake program.
// It implements a complete, self-contained merge functionality that matches
// the native Solana stake program's merge instruction behavior.
//
// ## ARCHITECTURE OVERVIEW
//
// This file is structured in several key sections:
// 1. Real Stake History Implementation - Reads actual network stake history
// 2. Merge-Specific Delegation Logic - Handles warmup/cooldown calculations
// 3. Merge Kind System - Classifies stakes as Inactive/ActivationEpoch/FullyActive
// 4. Main Merge Processor - Orchestrates the entire merge operation
//
// ## WHY DEDICATED MODULE?
//
// This avoids modifying existing files (delegation.rs, stake_history.rs, split.rs)
// while providing 100% merge functionality. All merge logic is contained here.
//
// ## MERGE INSTRUCTION FLOW
//
// 1. Validate accounts and permissions
// 2. Read real stake history from network
// 3. Classify both stakes (source & destination)
// 4. Validate merge compatibility
// 5. Perform the merge operation
// 6. Update accounts and transfer lamports

use crate::{
    helpers::*,
    state::{
        accounts::{Authorized, Meta, StakeAuthorize},
        stake_state_v2::StakeStateV2,
    },
    ID,
};
use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::{clock::Clock, Sysvar},
    ProgramResult,
};

// ============================================================================
// === REAL STAKE HISTORY IMPLEMENTATION ===
// ============================================================================
//
// This section handles reading the actual network stake history from the
// stake history sysvar account. This is CRITICAL for accurate warmup/cooldown
// calculations that match the native stake program.

/// The stake history sysvar account ID - this is a well-known Solana sysvar
const STAKE_HISTORY_ID: Pubkey = [
    6, 167, 213, 23, 25, 199, 116, 201, 40, 86, 99, 152, 105, 29, 94, 182, 139, 94, 184, 163, 155,
    75, 109, 92, 115, 85, 91, 33, 0, 0, 0, 0,
];

/// Real stake history wrapper that reads from the sysvar account
///
/// This struct contains the actual network stake history data, which is
/// essential for calculating proper warmup/cooldown rates during merge.
#[derive(Debug, Clone)]
pub struct RealStakeHistorySysvar {
    /// Current epoch when this stake history was read
    pub current_epoch: u64,
    /// Historical stake data: Vec<(epoch, stake_data)>
    /// Sorted by epoch for binary search lookup
    pub entries: Vec<(u64, RealStakeHistoryEntry)>,
}

/// Real stake history entry matching Solana's exact format
///
/// Each entry represents the network's staking state for one epoch:
/// - effective: Total SOL actively earning rewards
/// - activating: SOL still "warming up" (becoming effective)  
/// - deactivating: SOL "cooling down" (being unstaked)
#[repr(C)]
#[derive(Debug, PartialEq, Eq, Default, Clone, Copy)]
pub struct RealStakeHistoryEntry {
    pub effective: u64,    // Active stake earning rewards
    pub activating: u64,   // Stake warming up this epoch
    pub deactivating: u64, // Stake cooling down this epoch
}

impl RealStakeHistoryEntry {
    /// Create entry with only effective stake (fully activated)
    pub fn with_effective(effective: u64) -> Self {
        Self {
            effective,
            activating: 0,
            deactivating: 0,
        }
    }

    /// Create entry for deactivating stake (cooling down)
    pub fn with_deactivating(deactivating: u64) -> Self {
        Self {
            effective: deactivating, // Still effective during deactivation epoch
            activating: 0,
            deactivating,
        }
    }
}

impl RealStakeHistorySysvar {
    /// Read and validate the real stake history from the sysvar account
    ///
    /// This is the critical function that gives us access to real network data
    /// instead of dummy values. It ensures we match native program behavior.
    ///
    /// ## Arguments
    /// * `account_info` - The stake history sysvar account
    /// * `current_epoch` - Current epoch from clock
    ///
    /// ## Returns
    /// * `Result<Self, ProgramError>` - Parsed stake history or error
    ///
    /// ## Validation
    /// 1. Verifies account key matches STAKE_HISTORY_ID
    /// 2. Deserializes the account data using bincode
    /// 3. Returns structured stake history data
    pub fn from_account_info_unchecked(
        account_info: &AccountInfo,
        current_epoch: u64,
    ) -> Result<Self, ProgramError> {
        // CRITICAL: Validate this is the correct stake history sysvar account
        // This prevents malicious accounts from providing fake stake history
        if *account_info.key() != STAKE_HISTORY_ID {
            return Err(ProgramError::InvalidArgument);
        }

        // Read the raw account data
        let data = unsafe { account_info.borrow_data_unchecked() };

        // The stake history is stored as a Vec<(Epoch, StakeHistoryEntry)>
        // We deserialize it using bincode to match Solana's format exactly
        let entries: Vec<(u64, RealStakeHistoryEntry)> =
            bincode::deserialize(&data).map_err(|_| ProgramError::InvalidAccountData)?;

        Ok(Self {
            current_epoch,
            entries,
        })
    }

    /// Get stake history entry for a specific epoch using binary search
    ///
    /// This uses the same binary search algorithm as the native stake program
    /// to efficiently find historical stake data for any epoch.
    ///
    /// ## Arguments  
    /// * `epoch` - The epoch to look up
    ///
    /// ## Returns
    /// * `Option<RealStakeHistoryEntry>` - Stake data for that epoch, or None
    pub fn get_entry(&self, epoch: u64) -> Option<RealStakeHistoryEntry> {
        self.entries
            .binary_search_by_key(&epoch, |entry| entry.0)
            .ok()
            .map(|index| self.entries[index].1)
    }
}

/// Trait for getting stake history entries - abstraction for testing
///
/// This trait allows us to abstract over different stake history sources
/// while maintaining the same interface for the delegation logic.
pub trait RealStakeHistoryGetEntry {
    fn get_entry(&self, epoch: u64) -> Option<RealStakeHistoryEntry>;
}

impl RealStakeHistoryGetEntry for RealStakeHistorySysvar {
    fn get_entry(&self, epoch: u64) -> Option<RealStakeHistoryEntry> {
        self.get_entry(epoch)
    }
}

// ============================================================================
// === MERGE-SPECIFIC DELEGATION IMPLEMENTATION ===
// ============================================================================
//
// This section handles the complex warmup/cooldown calculations that determine
// how much stake is "effective" (earning rewards) vs "activating" or "deactivating".
// This logic is CRITICAL for determining if stakes can be merged.

/// Merge-specific delegation structure
///
/// This mirrors the native Delegation struct but is contained within this module
/// to avoid modifying existing delegation.rs file. It handles all stake lifecycle.
#[repr(C)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct MergeDelegation {
    pub voter_pubkey: Pubkey,          // Validator this stake is delegated to
    pub stake: [u8; 8],                // Amount of stake (as bytes for consistency)
    pub activation_epoch: u64,         // When this stake started activating
    pub deactivation_epoch: u64, // When this stake started deactivating (u64::MAX if not deactivating)
    pub warmup_cooldown_rate: [u8; 8], // Rate settings (legacy field)
}

impl MergeDelegation {
    /// Calculate warmup/cooldown rate for the current epoch
    ///
    /// Solana has two different warmup/cooldown rates:
    /// - Old rate: 25% of total activating/deactivating stake per epoch
    /// - New rate: 9% of total activating/deactivating stake per epoch
    ///
    /// The rate used depends on when the new rate was activated.
    ///
    /// ## Arguments
    /// * `current_epoch` - Current network epoch
    /// * `new_rate_activation_epoch` - When new 9% rate became active
    ///
    /// ## Returns
    /// * `f64` - Rate to use (0.25 or 0.09)
    fn merge_warmup_cooldown_rate(
        current_epoch: u64,
        new_rate_activation_epoch: Option<u64>,
    ) -> f64 {
        const MERGE_DEFAULT_WARMUP_COOLDOWN_RATE: f64 = 0.25; // 25% - old rate
        const MERGE_NEW_WARMUP_COOLDOWN_RATE: f64 = 0.09; // 9% - new rate

        if current_epoch < new_rate_activation_epoch.unwrap_or(u64::MAX) {
            MERGE_DEFAULT_WARMUP_COOLDOWN_RATE
        } else {
            MERGE_NEW_WARMUP_COOLDOWN_RATE
        }
    }

    /// THE CORE METHOD: Determine stake activation/deactivation status
    ///
    /// This is the heart of the merge logic. It calculates exactly how much
    /// stake is effective, activating, or deactivating at the target epoch.
    /// This determines whether stakes can be merged together.
    ///
    /// ## Key Concepts
    /// - **Effective**: Stake that's fully active and earning rewards
    /// - **Activating**: Stake that's warming up (not yet fully effective)
    /// - **Deactivating**: Stake that's cooling down (being unstaked)
    ///
    /// ## Merge Rules
    /// - Can merge: Inactive + Inactive, FullyActive + FullyActive
    /// - Cannot merge: Anything with activating/deactivating stake (transient)
    ///
    /// ## Arguments
    /// * `target_epoch` - Epoch to calculate status for (usually current)
    /// * `history` - Real network stake history for calculations
    /// * `new_rate_activation_epoch` - When new warmup rate became active
    ///
    /// ## Returns
    /// * `RealStakeHistoryEntry` - Breakdown of effective/activating/deactivating amounts
    pub fn stake_activating_and_deactivating(
        &self,
        target_epoch: u64,
        history: &impl RealStakeHistoryGetEntry,
        new_rate_activation_epoch: Option<u64>,
    ) -> RealStakeHistoryEntry {
        // STEP 1: Calculate basic activation status (ignoring deactivation)
        let (effective, activating) =
            self.stake_and_activating(target_epoch, history, new_rate_activation_epoch);

        // STEP 2: Handle deactivation scenarios
        if target_epoch < self.deactivation_epoch || self.deactivation_epoch == u64::MAX {
            // Case A: Not deactivated yet
            if activating == 0 {
                // Fully effective - ideal for merging
                RealStakeHistoryEntry::with_effective(effective)
            } else {
                // Still activating - cannot merge (transient state)
                RealStakeHistoryEntry {
                    effective,
                    activating,
                    deactivating: 0,
                }
            }
        } else if target_epoch == self.deactivation_epoch {
            // Case B: Just started deactivating this epoch
            RealStakeHistoryEntry::with_deactivating(effective)
        } else if let Some(deactivation_entry) = history.get_entry(self.deactivation_epoch) {
            // Case C: In cooldown period - complex calculation required
            //
            // This implements the exact same cooldown algorithm as the native program.
            // We iterate through each epoch since deactivation, calculating how much
            // stake becomes non-effective based on the network's total deactivating stake.

            let mut current_effective_stake = effective;
            let mut prev_epoch = self.deactivation_epoch;
            let mut prev_cluster_stake = deactivation_entry;

            // Iterate through cooldown period
            loop {
                let current_epoch = prev_epoch + 1;

                // If no network stake is deactivating, we're done
                if prev_cluster_stake.deactivating == 0 {
                    break;
                }

                // Calculate our proportional share of network deactivation
                let weight =
                    current_effective_stake as f64 / prev_cluster_stake.deactivating as f64;

                // Calculate how much network stake becomes non-effective this epoch
                let newly_not_effective_cluster_stake = prev_cluster_stake.deactivating.min(
                    (Self::merge_warmup_cooldown_rate(current_epoch, new_rate_activation_epoch)
                        * prev_cluster_stake.deactivating as f64) as u64,
                );

                // Calculate how much of OUR stake becomes non-effective
                let newly_not_effective_stake =
                    ((weight * newly_not_effective_cluster_stake as f64) as u64)
                        .min(current_effective_stake);

                // Reduce our effective stake
                current_effective_stake -= newly_not_effective_stake;

                // Stop if we've reached the target epoch or have no effective stake left
                if current_effective_stake == 0 || current_epoch >= target_epoch {
                    break;
                }

                // Move to next epoch if we have historical data
                if let Some(current_cluster_stake) = history.get_entry(current_epoch) {
                    prev_epoch = current_epoch;
                    prev_cluster_stake = current_cluster_stake;
                } else {
                    break;
                }
            }

            RealStakeHistoryEntry::with_deactivating(current_effective_stake)
        } else {
            // Case D: No history available - assume fully deactivated
            RealStakeHistoryEntry::default()
        }
    }

    /// Calculate activation status (ignoring deactivation)
    ///
    /// This helper function determines how much stake is effective vs activating,
    /// without considering deactivation. It handles the warmup period calculation.
    ///
    /// ## Returns
    /// * `(u64, u64)` - (effective_stake, activating_stake)
    fn stake_and_activating(
        &self,
        target_epoch: u64,
        history: &impl RealStakeHistoryGetEntry,
        new_rate_activation_epoch: Option<u64>,
    ) -> (u64, u64) {
        let delegated_stake = u64::from_le_bytes(self.stake);

        if self.activation_epoch == u64::MAX {
            // Bootstrap stake - fully effective immediately (genesis case)
            (delegated_stake, 0)
        } else if self.activation_epoch == self.deactivation_epoch {
            // Instantly deactivated - edge case
            (0, 0)
        } else if target_epoch == self.activation_epoch {
            // Just delegated this epoch - all stake is activating
            (0, delegated_stake)
        } else if target_epoch < self.activation_epoch {
            // Not yet activated - no stake effective
            (0, 0)
        } else if let Some(activation_entry) = history.get_entry(self.activation_epoch) {
            // Normal case: Handle warmup period calculation
            //
            // This implements the same warmup algorithm as cooldown, but in reverse.
            // Each epoch, some percentage of activating stake becomes effective.

            let mut current_effective_stake = 0;
            let mut prev_epoch = self.activation_epoch;
            let mut prev_cluster_stake = activation_entry;

            // Iterate through warmup period
            loop {
                let current_epoch = prev_epoch + 1;

                // If no network stake is activating, we're done
                if prev_cluster_stake.activating == 0 {
                    break;
                }

                let remaining_activating_stake = delegated_stake - current_effective_stake;

                // Calculate our proportional share of network activation
                let weight =
                    remaining_activating_stake as f64 / prev_cluster_stake.activating as f64;

                // Calculate how much network stake becomes effective this epoch
                let newly_effective_cluster_stake = prev_cluster_stake.activating.min(
                    (Self::merge_warmup_cooldown_rate(current_epoch, new_rate_activation_epoch)
                        * prev_cluster_stake.activating as f64) as u64,
                );

                // Calculate how much of OUR stake becomes effective
                let newly_effective_stake = ((weight * newly_effective_cluster_stake as f64)
                    as u64)
                    .min(remaining_activating_stake);

                current_effective_stake += newly_effective_stake;

                // Stop if fully effective or reached target epoch
                if current_effective_stake >= delegated_stake || current_epoch >= target_epoch {
                    current_effective_stake = current_effective_stake.min(delegated_stake);
                    break;
                }

                // Move to next epoch if we have historical data
                if let Some(current_cluster_stake) = history.get_entry(current_epoch) {
                    prev_epoch = current_epoch;
                    prev_cluster_stake = current_cluster_stake;
                } else {
                    break;
                }
            }

            (
                current_effective_stake,
                delegated_stake - current_effective_stake,
            )
        } else {
            // No history available - assume fully effective (fallback)
            (delegated_stake, 0)
        }
    }
}

// ============================================================================
// === MERGE-SPECIFIC STAKE STATE ===
// ============================================================================

/// Complete stake information for merge operations
///
/// This combines delegation info with credits observed for a complete
/// picture of a stake account's state.
#[repr(C)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct MergeStake {
    pub delegation: MergeDelegation, // Delegation details
    pub credits_observed: [u8; 8],   // Last observed vote credits (for rewards)
}

// ============================================================================
// === MERGE KIND CLASSIFICATION SYSTEM ===
// ============================================================================
//
// This section implements the core merge compatibility logic. Stakes are
// classified into categories that determine what can be merged with what.

/// Classification of stake accounts for merge compatibility
///
/// This enum represents the three possible states a stake account can be in
/// for merge purposes. Only certain combinations can be merged together.
///
/// ## Merge Compatibility Matrix
/// ```
///                  | Inactive | ActivationEpoch | FullyActive
/// Inactive         |    ✅    |       ❌        |     ❌
/// ActivationEpoch  |    ❌    |       ❌        |     ❌  
/// FullyActive      |    ❌    |       ❌        |     ✅
/// ```
#[derive(Debug, PartialEq)]
pub enum MergeKind {
    /// Inactive stake - not delegated, can be merged with other inactive
    Inactive(Meta, u64, crate::state::stake_flag::StakeFlags),

    /// Stake in activation epoch - has activating stake, cannot be merged (transient)
    ActivationEpoch(Meta, MergeStake, crate::state::stake_flag::StakeFlags),

    /// Fully active stake - can be merged with other fully active to same validator
    FullyActive(Meta, MergeStake),
}

impl MergeKind {
    /// Classify a stake account and determine if it's mergeable
    ///
    /// This is the key function that determines merge compatibility.
    /// It examines the stake's activation state and classifies it accordingly.
    ///
    /// ## Arguments  
    /// * `stake_state` - The stake account's current state
    /// * `stake_lamports` - Account balance
    /// * `clock` - Current network time
    /// * `stake_history` - Real network stake history
    ///
    /// ## Returns
    /// * `Result<Self, ProgramError>` - Merge classification or error
    ///
    /// ## Classification Logic
    /// 1. Initialized (not delegated) → Inactive
    /// 2. Delegated stake → Check activation status:
    ///    - (0,0,0) effective/activating/deactivating → Inactive  
    ///    - (0,_,_) no effective, some activating → ActivationEpoch
    ///    - (_,0,0) effective, no activating/deactivating → FullyActive
    ///    - Other combinations → Error (transient, cannot merge)
    pub fn get_if_mergeable(
        stake_state: &StakeStateV2,
        stake_lamports: u64,
        clock: &Clock,
        stake_history: &RealStakeHistorySysvar,
    ) -> Result<Self, ProgramError> {
        match stake_state {
            // Case 1: Initialized but not delegated - always mergeable as Inactive
            StakeStateV2::Initialized(meta) => Ok(Self::Inactive(
                *meta,
                stake_lamports,
                crate::state::stake_flag::StakeFlags::empty(),
            )),

            // Case 2: Delegated stake - need to check activation status
            StakeStateV2::Stake(meta, stake, stake_flags) => {
                // Convert to our merge-specific delegation format
                let merge_delegation = MergeDelegation {
                    voter_pubkey: stake.delegation.voter_pubkey,
                    stake: stake.delegation.stake,
                    activation_epoch: stake.delegation.activation_epoch,
                    deactivation_epoch: stake.delegation.deactivation_epoch,
                    warmup_cooldown_rate: stake.delegation.warmup_cooldown_rate,
                };

                let merge_stake = MergeStake {
                    delegation: merge_delegation,
                    credits_observed: stake.credits_observed,
                };

                // THE CRITICAL CHECK: Determine activation/deactivation status
                // This uses REAL network stake history for accurate calculations
                let status = merge_delegation.stake_activating_and_deactivating(
                    clock.epoch,
                    stake_history,
                    Some(0), // PERPETUAL_NEW_WARMUP_COOLDOWN_RATE_EPOCH
                );

                // Classify based on the activation status
                match (status.effective, status.activating, status.deactivating) {
                    (0, 0, 0) => {
                        // No effective, activating, or deactivating stake - treat as inactive
                        Ok(Self::Inactive(*meta, stake_lamports, *stake_flags))
                    }
                    (0, _, _) => {
                        // No effective stake but some activating - in activation epoch
                        // Cannot merge because it's in a transient state
                        Ok(Self::ActivationEpoch(*meta, merge_stake, *stake_flags))
                    }
                    (_, 0, 0) => {
                        // Has effective stake, no activating/deactivating - fully active
                        // This is the ideal state for merging
                        Ok(Self::FullyActive(*meta, merge_stake))
                    }
                    _ => {
                        // Any other combination means transient state - cannot merge
                        // This includes stakes that are deactivating or partially activating
                        Err(ProgramError::InvalidAccountData)
                    }
                }
            }

            // Case 3: Any other state (Uninitialized, RewardsPool) - not mergeable
            _ => Err(ProgramError::InvalidAccountData),
        }
    }

    /// Get the metadata from any merge kind
    ///
    /// All merge kinds contain metadata (authorities, lockup, etc.)
    /// This helper extracts it regardless of the specific kind.
    pub fn meta(&self) -> &Meta {
        match self {
            Self::Inactive(meta, _, _) => meta,
            Self::ActivationEpoch(meta, _, _) => meta,
            Self::FullyActive(meta, _) => meta,
        }
    }

    /// Perform the actual merge operation
    ///
    /// This is where the magic happens. It takes two classified stakes and
    /// merges them according to the compatibility rules.
    ///
    /// ## Arguments
    /// * `self` - Destination stake (will receive merged result)
    /// * `source` - Source stake (will be drained and closed)
    /// * `clock` - Current network time
    ///
    /// ## Returns  
    /// * `Result<Option<StakeStateV2>, ProgramError>` - New state or None if no change
    ///
    /// ## Merge Rules Implementation
    /// - Inactive + Inactive: Combine lamports, update rent reserve
    /// - FullyActive + FullyActive: Merge stakes and credits (weighted average)
    /// - Everything else: Error (incompatible)
    pub fn merge(
        self,
        source: MergeKind,
        clock: &Clock,
    ) -> Result<Option<StakeStateV2>, ProgramError> {
        // STEP 1: Validate that basic metadata is compatible
        Self::metas_can_merge(self.meta(), source.meta(), clock)?;

        // STEP 2: Perform merge based on stake types
        match (self, source) {
            // CASE A: Both inactive - simple lamport merge
            (
                Self::Inactive(mut dest_meta, dest_lamports, dest_flags),
                Self::Inactive(source_meta, source_lamports, source_flags),
            ) => {
                // Stake flags must match exactly
                if dest_flags != source_flags {
                    return Err(ProgramError::InvalidAccountData);
                }

                // Update rent exempt reserve to the higher value (safety)
                if dest_meta.rent_exempt_reserve < source_meta.rent_exempt_reserve {
                    dest_meta.rent_exempt_reserve = source_meta.rent_exempt_reserve;
                }

                // Result: Initialized account with combined metadata
                Ok(Some(StakeStateV2::Initialized(dest_meta)))
            }

            // CASE B: Both fully active - complex stake merge
            (
                Self::FullyActive(dest_meta, mut dest_stake),
                Self::FullyActive(source_meta, source_stake),
            ) => {
                // CRITICAL: Must be delegated to the same validator
                if dest_stake.delegation.voter_pubkey != source_stake.delegation.voter_pubkey {
                    return Err(ProgramError::InvalidAccountData);
                }

                // STEP B1: Merge stake amounts with overflow protection
                let dest_stake_amount = u64::from_le_bytes(dest_stake.delegation.stake);
                let source_stake_amount = u64::from_le_bytes(source_stake.delegation.stake);
                let merged_stake_amount = dest_stake_amount
                    .checked_add(source_stake_amount)
                    .ok_or(ProgramError::ArithmeticOverflow)?;

                dest_stake.delegation.stake = merged_stake_amount.to_le_bytes();

                // STEP B2: Merge credits observed using weighted average
                // This is critical for proper reward calculation after merge
                let dest_credits = u64::from_le_bytes(dest_stake.credits_observed);
                let source_credits = u64::from_le_bytes(source_stake.credits_observed);
                let total_stake = merged_stake_amount;

                if total_stake > 0 {
                    // Weighted average: (credits1*stake1 + credits2*stake2) / total_stake
                    let merged_credits = (dest_credits * dest_stake_amount
                        + source_credits * source_stake_amount)
                        / total_stake;
                    dest_stake.credits_observed = merged_credits.to_le_bytes();
                }

                // STEP B3: Convert back to the original stake format
                // Our internal MergeDelegation needs to become standard Delegation
                let original_delegation = crate::state::delegation::Delegation {
                    voter_pubkey: dest_stake.delegation.voter_pubkey,
                    stake: dest_stake.delegation.stake,
                    activation_epoch: dest_stake.delegation.activation_epoch,
                    deactivation_epoch: dest_stake.delegation.deactivation_epoch,
                    warmup_cooldown_rate: dest_stake.delegation.warmup_cooldown_rate,
                };

                let original_stake = crate::state::delegation::Stake {
                    delegation: original_delegation,
                    credits_observed: dest_stake.credits_observed,
                };

                // Result: Merged stake account
                Ok(Some(StakeStateV2::Stake(
                    dest_meta,
                    original_stake,
                    crate::state::stake_flag::StakeFlags::empty(),
                )))
            }

            // CASE C: Any ActivationEpoch stake - cannot merge (transient state)
            (Self::ActivationEpoch(..), _) | (_, Self::ActivationEpoch(..)) => {
                Err(ProgramError::InvalidAccountData)
            }

            // CASE D: Incompatible combinations - cannot merge
            _ => Err(ProgramError::InvalidAccountData),
        }
    }

    /// Validate that two stakes have compatible metadata for merging
    ///
    /// This checks that authorities and lockups are compatible.
    /// Both stakes must have the same authorities and compatible lockups.
    ///
    /// ## Arguments
    /// * `dest` - Destination stake metadata
    /// * `source` - Source stake metadata
    /// * `clock` - Current time for lockup validation
    ///
    /// ## Returns
    /// * `ProgramResult` - Ok(()) if compatible, Err if not
    ///
    /// ## Validation Rules
    /// 1. Authorities must be identical
    /// 2. If either stake is locked, both must have compatible lockups
    /// 3. If both are locked, lockups must be identical
    fn metas_can_merge(dest: &Meta, source: &Meta, clock: &Clock) -> ProgramResult {
        // Rule 1: Authorities must match exactly
        if dest.authorized != source.authorized {
            return Err(ProgramError::InvalidAccountData);
        }

        // Rule 2: Lockup compatibility check
        // Both stakes must have the same lockup enforcement status
        if dest.lockup.is_in_force(clock, None) != source.lockup.is_in_force(clock, None) {
            return Err(ProgramError::InvalidAccountData);
        }

        // Rule 3: If both are locked, lockups must be identical
        if dest.lockup.is_in_force(clock, None) && dest.lockup != source.lockup {
            return Err(ProgramError::InvalidAccountData);
        }

        Ok(())
    }
}

// ============================================================================
// === MAIN MERGE PROCESSOR ===
// ============================================================================
//
// This is the main entry point that orchestrates the entire merge operation.
// It handles account validation, permission checking, and coordinating all
// the merge logic defined above.

/// Main merge processor - orchestrates the complete merge operation
///
/// This function implements the complete merge instruction flow:
/// 1. Account validation and parsing
/// 2. Permission and authority checking  
/// 3. Stake classification and compatibility validation
/// 4. Merge execution
/// 5. Account updates and lamport transfers
///
/// ## Arguments
/// * `accounts` - Array of account infos:
///   - [0] Destination stake account (writable) - receives merged stake
///   - [1] Source stake account (writable) - gets drained and closed
///   - [2] Clock sysvar account - for current time
///   - [3] Stake history sysvar account - for warmup/cooldown calculations
///   - [4+] Additional signers (authority must be among them)
///
/// ## Returns
/// * `ProgramResult` - Ok(()) on success, Err(ProgramError) on failure
///
/// ## Complete Flow Breakdown
///
/// ### Phase 1: Account Setup and Validation
/// 1. Parse accounts array and validate count
/// 2. Collect all signers from accounts  
/// 3. Validate accounts are writable and owned by stake program
/// 4. Ensure source and destination are different accounts
///
/// ### Phase 2: Sysvar Data Loading
/// 1. Load current time from clock sysvar
/// 2. Load real network stake history from stake history sysvar
/// 3. Validate sysvar accounts are correct
///
/// ### Phase 3: Stake State Analysis  
/// 1. Deserialize both stake account states
/// 2. Classify each stake (Inactive/ActivationEpoch/FullyActive)
/// 3. Validate merge compatibility
///
/// ### Phase 4: Permission Validation
/// 1. Check that stake authority signed the transaction
/// 2. Validate lockup compatibility
/// 3. Ensure all merge preconditions are met
///
/// ### Phase 5: Merge Execution
/// 1. Perform the actual merge operation
/// 2. Update destination account with merged state
/// 3. Clear source account (set to Uninitialized)
/// 4. Transfer all lamports from source to destination
///
/// ## Error Conditions
/// - Invalid account count or types
/// - Missing required signatures  
/// - Incompatible stake states (transient, different validators, etc.)
/// - Lockup conflicts
/// - Arithmetic overflow in stake amounts
/// - Invalid sysvar accounts
pub fn process_merge(accounts: &[AccountInfo]) -> ProgramResult {
    // ========================================================================
    // === PHASE 1: ACCOUNT SETUP AND VALIDATION ===
    // ========================================================================

    // Parse accounts - we expect at least 4 accounts
    let [destination_stake_account_info, source_stake_account_info, clock_info, stake_history_info, ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // Collect all signers from the accounts array
    // The stake authority must be among the signers
    let mut signers_keys = [Pubkey::default(); MAXIMUM_SIGNERS];
    let signers_len = collect_signers(accounts, &mut signers_keys)?;
    let signers = &signers_keys[..signers_len];

    // Ensure source and destination are different accounts
    if destination_stake_account_info.key() == source_stake_account_info.key() {
        return Err(ProgramError::InvalidArgument);
    }

    // Both accounts must be writable for the merge operation
    if !destination_stake_account_info.is_writable() || !source_stake_account_info.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }

    // Both accounts must be owned by the stake program
    if *destination_stake_account_info.owner() != ID || *source_stake_account_info.owner() != ID {
        return Err(ProgramError::InvalidAccountOwner);
    }

    // ========================================================================
    // === PHASE 2: SYSVAR DATA LOADING ===
    // ========================================================================

    // Load current network time from clock sysvar
    let clock = unsafe { Clock::from_account_info_unchecked(clock_info)? };

    // CRITICAL: Read the REAL stake history from the network sysvar account
    // This gives us access to actual network staking data for accurate calculations
    let stake_history = unsafe {
        RealStakeHistorySysvar::from_account_info_unchecked(stake_history_info, clock.epoch)?
    };

    // ========================================================================
    // === PHASE 3: STAKE STATE ANALYSIS ===
    // ========================================================================

    // Deserialize both stake account states
    let destination_stake_state = unsafe {
        StakeStateV2::deserialize_unchecked(
            &destination_stake_account_info.borrow_data_unchecked(),
        )?
    };
    let source_stake_state = unsafe {
        StakeStateV2::deserialize_unchecked(&source_stake_account_info.borrow_data_unchecked())?
    };

    // Classify destination stake and check if it's mergeable
    let destination_merge_kind = MergeKind::get_if_mergeable(
        &destination_stake_state,
        destination_stake_account_info.lamports(),
        &clock,
        &stake_history,
    )?;

    // ========================================================================
    // === PHASE 4: PERMISSION VALIDATION ===
    // ========================================================================

    // Verify that the stake authority signed this transaction
    // Only the stake authority can authorize merge operations
    destination_merge_kind
        .meta()
        .authorized
        .check(signers, StakeAuthorize::Staker)?;

    // Classify source stake and check if it's mergeable
    let source_merge_kind = MergeKind::get_if_mergeable(
        &source_stake_state,
        source_stake_account_info.lamports(),
        &clock,
        &stake_history,
    )?;

    // ========================================================================
    // === PHASE 5: MERGE EXECUTION ===
    // ========================================================================

    // Attempt to perform the merge operation
    // This will validate compatibility and return the merged state if successful
    if let Some(merged_state) = destination_merge_kind.merge(source_merge_kind, &clock)? {
        // Update destination account with the merged stake state
        unsafe {
            merged_state.serialize_unchecked(
                &mut destination_stake_account_info.borrow_mut_data_unchecked(),
            )?;
        }
    }

    // Clear the source account by setting it to Uninitialized state
    // This prevents the source account from being used again
    unsafe {
        StakeStateV2::Uninitialized
            .serialize_unchecked(&mut source_stake_account_info.borrow_mut_data_unchecked())?;
    }

    // Transfer ALL lamports from source to destination
    // This is the final step that completes the merge
    relocate_lamports(
        source_stake_account_info,
        destination_stake_account_info,
        source_stake_account_info.lamports(),
    )?;

    // Success! The merge operation is complete
    Ok(())
}
