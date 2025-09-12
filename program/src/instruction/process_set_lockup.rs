use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::{clock::Clock, Sysvar},
    ProgramResult,
};

use crate::{
    helpers::{collect_signers, get_stake_state, set_stake_state, constant::MAXIMUM_SIGNERS},
    state::{
        accounts::SetLockupData,         // parsed instruction payload with Option<i64>, Option<u64>, Option<Pubkey>
        stake_state_v2::StakeStateV2,
        state::Meta,                     // your Meta carrying Authorized + Lockup
    },
};

/// Native-compatible SetLockup:
/// - stake account is the first account; other accounts are only used to gather signers
/// - no PDA/system-program accounts are required or created
/// - if lockup in force => custodian must sign
///   else => withdraw authority must sign
/// - apply fields directly (no monotonic checks; no "must have at least one" constraint)
pub fn process_set_lockup(accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult {
    // Native asserts: first account is the stake account
    let [stake_ai, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // The stake account must be owned by this program and writable
    if *stake_ai.owner() != crate::ID || !stake_ai.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }

    // Parse the instruction payload (three Option fields)
    // Your SetLockupData::instruction_data(...) should return a value with:
    //   { unix_timestamp: Option<i64>, epoch: Option<u64>, custodian: Option<Pubkey> }
    let args = SetLockupData::instruction_data(instruction_data);

    // Native reads the sysvar directly, not from an account param
    let clock = Clock::get()?;

    // Collect all signer keys from *all* provided accounts
    let mut buf = [Pubkey::default(); MAXIMUM_SIGNERS];
    let n = collect_signers(accounts, &mut buf)?;
    let signers = &buf[..n];
    let signed = |pk: &Pubkey| signers.iter().any(|s| s == pk);

    // Load stake state, update lockup policy + fields, then write back
    match get_stake_state(stake_ai)? {
        StakeStateV2::Initialized(mut meta) => {
            apply_lockup_update(&mut meta, &args, &clock, &signed)?;
            set_stake_state(stake_ai, &StakeStateV2::Initialized(meta))?;
        }
        StakeStateV2::Stake(mut meta, stake, flags) => {
            apply_lockup_update(&mut meta, &args, &clock, &signed)?;
            set_stake_state(stake_ai, &StakeStateV2::Stake(meta, stake, flags))?;
        }
        _ => return Err(ProgramError::InvalidAccountData),
    }

    Ok(())
}

/// Native set_lockup policy:
/// - If current lockup is in force (time or epoch) => existing custodian must have signed
/// - Else => withdraw authority must have signed
/// - Then apply optional fields directly
fn apply_lockup_update(
    meta: &mut Meta,
    args: &SetLockupData,
    clock: &Clock,
    signed: &impl Fn(&Pubkey) -> bool,
) -> ProgramResult {
    // Use your Lockup::is_in_force with *no* custodian bypass to determine if it's active
    let lockup_active = meta.lockup.is_in_force(clock, None);

    if lockup_active {
        // custodian must sign if lockup is currently in force
        if !signed(&meta.lockup.custodian) {
            return Err(ProgramError::MissingRequiredSignature);
        }
    } else {
        // otherwise withdraw authority must sign
        if !signed(&meta.authorized.withdrawer) {
            return Err(ProgramError::MissingRequiredSignature);
        }
    }

    // Apply fields exactly like native (no monotonic constraint, no "must have one" check)
    if let Some(ts) = args.unix_timestamp {
    meta.lockup.unix_timestamp = ts;            // <-- no to_le_bytes()
    }
   if let Some(ep) = args.epoch {
    meta.lockup.epoch = ep;                     // <-- no to_le_bytes()
}
    if let Some(cust) = args.custodian {
        meta.lockup.custodian = cust;
    }

    Ok(())
}