use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::clock::Clock,
    ProgramResult,
};

use crate::{
    helpers::{collect_signers, get_stake_state, set_stake_state, authorize_update, MAXIMUM_SIGNERS},
    state::{stake_state_v2::StakeStateV2, StakeAuthorize},
};

/// Authorize (checked) instruction
/// Accounts (4 + optional custodian):
///   0. [writable] Stake account (must be owned by stake program)
///   1. [sysvar]   Clock
///   2. []         Old stake/withdraw authority (presence only; no strict signer requirement here)
///   3. [signer]   New stake/withdraw authority
///   4. [optional signer] Custodian (needed only if lockup is in force)
pub fn process_authorize_checked(
    accounts: &[AccountInfo],
    authority_type: StakeAuthorize,
) -> ProgramResult {
    // Expected accounts: 4 (1 sysvar)
    if accounts.len() < 4 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    // stake, clock, _old_authority (ignored for signer checks here), new_authority, [maybe custodian, ...]
    let [stake_ai, clock_ai, _old_auth_ai, new_auth_ai, rest @ ..] = accounts else {
        return Err(ProgramError::InvalidAccountData);
    };

    // Basic safety checks
    if *stake_ai.owner() != crate::ID || !stake_ai.is_writable() {
        return Err(ProgramError::IncorrectProgramId);
    }
    if clock_ai.key() != &pinocchio::sysvars::clock::CLOCK_ID {
        return Err(ProgramError::InvalidArgument);
    }

    // New authority must be a signer
    if !new_auth_ai.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Optional custodian (only required if lockup is in force; the policy helper will decide)
    let maybe_lockup_authority: Option<&AccountInfo> = rest.first();

    // Load clock
    let clock = unsafe { Clock::from_account_info_unchecked(clock_ai)? };

    // Collect all transaction signers
    let mut signers_buf = [Pubkey::default(); MAXIMUM_SIGNERS];
    let n = collect_signers(accounts, &mut signers_buf)?;
    let signers = &signers_buf[..n];

    // New authority comes from the 4th account (not from instruction data in the checked variant)
    let new_authorized: Pubkey = *new_auth_ai.key();

    // Enforce that the old authority (account 2) signed and matches current meta
    let state = get_stake_state(stake_ai)?;
    let required_old = match &state {
        StakeStateV2::Initialized(meta) => match authority_type {
            StakeAuthorize::Staker => meta.authorized.staker,
            StakeAuthorize::Withdrawer => meta.authorized.withdrawer,
        },
        StakeStateV2::Stake(meta, _, _) => match authority_type {
            StakeAuthorize::Staker => meta.authorized.staker,
            StakeAuthorize::Withdrawer => meta.authorized.withdrawer,
        },
        _ => return Err(ProgramError::InvalidAccountData),
    };
    if _old_auth_ai.key() != &required_old || !_old_auth_ai.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Load -> authorize -> store
    match state {
        StakeStateV2::Initialized(mut meta) => {
            authorize_update(
                &mut meta,
                new_authorized,
                authority_type,
                signers,
                maybe_lockup_authority,
                &clock,
            )?;
            set_stake_state(stake_ai, &StakeStateV2::Initialized(meta))?;
        }
        StakeStateV2::Stake(mut meta, stake, flags) => {
            authorize_update(
                &mut meta,
                new_authorized,
                authority_type,
                signers,
                maybe_lockup_authority,
                &clock,
            )?;
            set_stake_state(stake_ai, &StakeStateV2::Stake(meta, stake, flags))?;
        }
        _ => return Err(ProgramError::InvalidAccountData),
    }

    Ok(())
}
