#![allow(clippy::result_large_err)]

use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    ProgramResult,
};

use crate::{
    helpers::{get_stake_state, set_stake_state},
    state::{
        stake_state_v2::StakeStateV2,
        state::{Lockup, Meta},
        accounts::Authorized,
    },
};

pub fn process_initialize_checked(accounts: &[AccountInfo]) -> ProgramResult {
    // Native requires: 4 accounts (stake, rent, stake_authority, withdraw_authority)
    if accounts.len() < 4 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let [stake_ai, rent_ai, stake_auth_ai, withdraw_auth_ai, ..] = accounts else {
        return Err(ProgramError::InvalidAccountData);
    };

    // Withdraw authority MUST sign (native)
    if !withdraw_auth_ai.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Basic stake account checks
    if !stake_ai.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }
    // get_stake_state() will also enforce program owner; we keep this explicit check helpful
    // but not strictly required if you prefer to rely solely on get_stake_state().
    // if *stake_ai.owner() != crate::ID {
    //     return Err(ProgramError::InvalidAccountOwner);
    // }

    // Rent sysvar must be passed (native uses from_account_info; here assert presence and read Rent)
    if rent_ai.key() != &pinocchio::sysvars::rent::RENT_ID {
        return Err(ProgramError::InvalidArgument);
    }
    let rent = Rent::get()?; // Pinocchio Sysvar access

    // Ensure the stake account is currently Uninitialized (native do_initialize does this)
    match get_stake_state(stake_ai)? {
        StakeStateV2::Uninitialized => {
            // Compute the rent-exempt reserve for the account's current data length
            let required_rent = rent.minimum_balance(stake_ai.data_len());

            // Ensure the account has enough lamports for rent exemption (native behavior)
            if stake_ai.lamports() < required_rent {
                return Err(ProgramError::InsufficientFunds);
            }

            // Build Authorized (staker from account #2, withdrawer from account #3)
            let authorized = Authorized {
                staker: *stake_auth_ai.key(),
                withdrawer: *withdraw_auth_ai.key(),
            };

            // Build Meta with default lockup
            // NOTE: If your `Meta.rent_exempt_reserve` is `u64` instead of `[u8; 8]`,
            //       assign `required_rent` directly (remove `.to_le_bytes()`).
            let meta = Meta {
                rent_exempt_reserve: required_rent.to_le_bytes(),
                authorized,
                lockup: Lockup::default(),
            };

            // Write Initialized state
            set_stake_state(stake_ai, &StakeStateV2::Initialized(meta))?;
            Ok(())
        }
        // Already initialized/active/etc. -> not allowed to initialize
        _ => Err(ProgramError::InvalidAccountData),
    }
}