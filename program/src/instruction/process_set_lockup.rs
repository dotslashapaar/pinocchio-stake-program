use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::{clock::Clock, Sysvar},
    ProgramResult,
};

use crate::{
    helpers::{collect_signers, next_account_info},
    helpers::utils::{get_stake_state, set_stake_state},
    helpers::constant::MAXIMUM_SIGNERS,
    state::{accounts::SetLockupData, stake_state_v2::StakeStateV2, state::Meta},
};

pub fn process_set_lockup(accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult {
    // Match native’s iteration/shape:
    // native asserts: 1 account (stake), but additional accounts may be supplied
    let account_info_iter = &mut accounts.iter();
    let stake_account_info = next_account_info(account_info_iter)?;
    // (Native ignores/permits additional accounts; we use them only to collect signers.)

    // Parse payload into optional fields (no “must set at least one” rule in native)
    let args = SetLockupData::instruction_data(instruction_data);

    // Native reads the clock sysvar directly (no clock account is required)
    let clock = Clock::get()?;

    // Collect *all* signers from all provided accounts (native behavior)
    let mut signer_buf = [Pubkey::default(); MAXIMUM_SIGNERS];
    let n = collect_signers(accounts, &mut signer_buf)?;
    let signers = &signer_buf[..n];

    // Owner and size checks are performed by get_stake_state(); writable is enforced by set_stake_state
    match get_stake_state(stake_account_info)? {
        StakeStateV2::Initialized(mut meta) => {
            apply_lockup_update(&mut meta, &args, &clock, signers)?;
            set_stake_state(stake_account_info, &StakeStateV2::Initialized(meta))
        }
        StakeStateV2::Stake(mut meta, stake, stake_flags) => {
            apply_lockup_update(&mut meta, &args, &clock, signers)?;
            set_stake_state(
                stake_account_info,
                &StakeStateV2::Stake(meta, stake, stake_flags),
            )
        }
        _ => Err(ProgramError::InvalidAccountData),
    }
}

/// Exactly the native gating in `Meta::set_lockup`:
/// - If lockup is in force → current custodian must have signed
/// - Else → current withdraw authority must have signed
/// Then apply any provided fields as-is.
fn apply_lockup_update(
    meta: &mut Meta,
    args: &SetLockupData,
    clock: &Clock,
    signers: &[Pubkey],
) -> ProgramResult {
    let signed = |pk: &Pubkey| signers.iter().any(|s| s == pk);

    // Lockup in force? (native passes None here; no custodian bypass)
    let in_force = meta.lockup.is_in_force(clock, None);

    if in_force {
        if !signed(&meta.lockup.custodian) {
            return Err(ProgramError::MissingRequiredSignature);
        }
    } else if !signed(&meta.authorized.withdrawer) {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Apply optional fields like native (no monotonicity check, no “must set one”)
    if let Some(ts) = args.unix_timestamp {
        // If your Lockup fields are numeric (i64), keep this:
        meta.lockup.unix_timestamp = ts;
        // If your Lockup uses [u8; 8], do: meta.lockup.unix_timestamp = ts.to_le_bytes();
    }
    if let Some(ep) = args.epoch {
        meta.lockup.epoch = ep;
        // If [u8; 8] layout: meta.lockup.epoch = ep.to_le_bytes();
    }
    if let Some(cust) = args.custodian {
        meta.lockup.custodian = cust;
    }

    Ok(())
}