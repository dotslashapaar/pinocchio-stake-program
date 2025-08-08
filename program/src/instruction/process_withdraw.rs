use crate::{
    state::{
        stake_state_v2::StakeStateV2,
        state::{Lockup, Meta},
        delegation::Stake,
        accounts::StakeAuthorize,
    },
    helpers::MAXIMUM_SIGNERS,
};
use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::{clock::Clock, Sysvar},
    ProgramResult,
};

/// Withdraw lamports from a stake account with proper security checks
/// 
/// Handles three account types:
/// - Uninitialized: Anyone can withdraw with account signature
/// - Initialized: Withdrawer authority required, respects lockup
/// - Delegated: Complex rules based on stake status and lockup
pub fn process_withdraw(accounts: &[AccountInfo], withdraw_lamports: u64) -> ProgramResult {
    // Extract accounts
    let [source_stake_account, destination_account, clock_sysvar, _stake_history_sysvar, withdraw_authority, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let custodian_authority = accounts.get(5);

    // Verify withdraw authority is signing
    if !withdraw_authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Get current blockchain time and epoch
    let clock = Clock::from_account_info(clock_sysvar)?;

    // Collect signers (no heap allocation)
    let mut signers_array = [Pubkey::default(); MAXIMUM_SIGNERS];
    let signers_count = collect_signers_fixed(
        withdraw_authority, 
        custodian_authority, 
        &mut signers_array
    )?;
    let signers = &signers_array[..signers_count];

    // Get custodian if present and signing
    let custodian = custodian_authority
        .filter(|c| c.is_signer())
        .map(|c| *c.key());

    // Get mutable access to stake account data
    let mut stake_data = source_stake_account.try_borrow_mut_data()?;
    let stake_state = StakeStateV2::deserialize(&stake_data)?;

    // Determine withdrawal constraints based on stake state
    let (meta, reserve, is_staked) = match &stake_state {
        StakeStateV2::Stake(meta, stake, _) => {
            // Verify withdrawer authority for delegated stakes
            check_authority(&meta.authorized, signers, StakeAuthorize::Withdrawer)?;
            
            // Calculate locked stake amount
            let staked = if clock.epoch >= stake.delegation.deactivation_epoch {
                // Stake is deactivating - calculate remaining stake
                calculate_remaining_stake(stake, clock.epoch)
            } else {
                // Stake is active - all delegated lamports are locked
                u64::from_le_bytes(stake.delegation.stake)
            };

            let reserve = u64::from_le_bytes(meta.rent_exempt_reserve);
            (meta, staked + reserve, staked != 0)
        }
        StakeStateV2::Initialized(meta) => {
            // Verify withdrawer authority for initialized accounts
            check_authority(&meta.authorized, signers, StakeAuthorize::Withdrawer)?;
            
            let reserve = u64::from_le_bytes(meta.rent_exempt_reserve);
            (meta, reserve, false)
        }
        StakeStateV2::Uninitialized => {
            // For uninitialized accounts, the account itself must sign
            if !contains_pubkey(signers, source_stake_account.key()) {
                return Err(ProgramError::MissingRequiredSignature);
            }
            // Create a default Meta for uninitialized accounts
            let default_meta = &Meta::default();
            (default_meta, 0, false)
        }
        StakeStateV2::RewardsPool => {
            return Err(ProgramError::InvalidAccountData);
        }
    };

    // Check if lockup prevents withdrawal
    if is_lockup_in_force(&meta.lockup, &clock, custodian)? {
        return Err(ProgramError::Custom(0x10)); // Lockup in force error
    }

    let stake_account_lamports = source_stake_account.lamports();
    
    if withdraw_lamports == stake_account_lamports {
        // Complete withdrawal - closing account
        if is_staked {
            return Err(ProgramError::InsufficientFunds); // Cannot close active stake
        }
        // Mark account as uninitialized
        let new_state = StakeStateV2::Uninitialized;
        new_state.serialize(&mut *stake_data)?;
    } else {
        // Partial withdrawal - check reserve requirements
        if withdraw_lamports + reserve > stake_account_lamports {
            return Err(ProgramError::InsufficientFunds);
        }
    }

    // Release data borrow before lamport transfer
    drop(stake_data);

    // Transfer lamports
    transfer_lamports(source_stake_account, destination_account, withdraw_lamports)?;

    Ok(())
}

/// Collect signers into fixed-size array
fn collect_signers_fixed(
    withdraw_authority: &AccountInfo,
    custodian_authority: Option<&AccountInfo>,
    signers_array: &mut [Pubkey; MAXIMUM_SIGNERS],
) -> Result<usize, ProgramError> {
    let mut count = 0;
    
    if withdraw_authority.is_signer() && count < MAXIMUM_SIGNERS {
        signers_array[count] = *withdraw_authority.key();
        count += 1;
    }
    
    if let Some(custodian) = custodian_authority {
        if custodian.is_signer() && count < MAXIMUM_SIGNERS {
            signers_array[count] = *custodian.key();
            count += 1;
        }
    }
    
    Ok(count)
}

/// Check if slice contains specific pubkey
fn contains_pubkey(signers: &[Pubkey], key: &Pubkey) -> bool {
    signers.iter().any(|signer| signer == key)
}

/// Verify signer has required authority
fn check_authority(
    authorized: &crate::state::accounts::Authorized,
    signers: &[Pubkey],
    authority_type: StakeAuthorize,
) -> ProgramResult {
    let required_key = match authority_type {
        StakeAuthorize::Staker => &authorized.staker,
        StakeAuthorize::Withdrawer => &authorized.withdrawer,
    };
    
    if contains_pubkey(signers, required_key) {
        Ok(())
    } else {
        Err(ProgramError::MissingRequiredSignature)
    }
}

/// Check if lockup is currently preventing withdrawals
fn is_lockup_in_force(
    lockup: &Lockup,
    clock: &Clock,
    custodian: Option<Pubkey>,
) -> Result<bool, ProgramError> {
    // Check if both time and epoch constraints have passed
    let time_passed = clock.unix_timestamp >= lockup.unix_timestamp;
    let epoch_passed = clock.epoch >= lockup.epoch;
    
    if time_passed && epoch_passed {
        return Ok(false); // Lockup expired
    }
    
    // Check if custodian is bypassing lockup
    if let Some(custodian_key) = custodian {
        if custodian_key == lockup.custodian {
            return Ok(false); // Custodian bypass
        }
    }
    
    Ok(true) // Lockup is active
}

/// Calculate remaining stake during deactivation cooldown
fn calculate_remaining_stake(stake: &Stake, current_epoch: u64) -> u64 {
    let deactivation_epoch = stake.delegation.deactivation_epoch;
    let stake_amount = u64::from_le_bytes(stake.delegation.stake);
    
    if current_epoch >= deactivation_epoch {
        let epochs_since_deactivation = current_epoch.saturating_sub(deactivation_epoch);
        
        // Simple cooldown: 1 epoch period
        if epochs_since_deactivation >= 1 {
            0 // Fully cooled down
        } else {
            stake_amount // Still cooling down
        }
    } else {
        stake_amount // Not yet deactivated
    }
}

/// Transfer lamports between accounts safely
fn transfer_lamports(
    from: &AccountInfo,
    to: &AccountInfo,
    lamports: u64,
) -> ProgramResult {
    if from.lamports() < lamports {
        return Err(ProgramError::InsufficientFunds);
    }
    
    **from.try_borrow_mut_lamports()? = from.lamports()
        .checked_sub(lamports)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    
    **to.try_borrow_mut_lamports()? = to.lamports()
        .checked_add(lamports)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    
    Ok(())
}
