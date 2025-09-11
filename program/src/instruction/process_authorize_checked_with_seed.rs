use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::clock::Clock,
    ProgramResult,
};

use crate::{
    helpers::{collect_signers, get_stake_state, set_stake_state, MAXIMUM_SIGNERS},
    // Centralized policy checks: staker/withdrawer auth + lockup/custodian
    helpers::authorize_update,
    state::{
        accounts::AuthorizeCheckedWithSeedData,
        stake_state_v2::StakeStateV2,
        StakeAuthorize,
    },
};

/// Recreates `Pubkey::create_with_seed(base, seed, owner)` in Pinocchio:
/// derived = sha256(base || seed || owner)
fn derive_with_seed_compat(base: &Pubkey, seed: &[u8], owner: &Pubkey) -> Result<Pubkey, ProgramError> {
    // Native restricts seed to <= 32 bytes
    if seed.len() > 32 {
        return Err(ProgramError::InvalidInstructionData);
    }

    let mut buf = [0u8; 32 + 32 + 32]; // base(32) + seed(<=32) + owner(32)
    let mut off = 0usize;

    // base
    buf[off..off + 32].copy_from_slice(&base[..]);
    off += 32;

    // seed
    buf[off..off + seed.len()].copy_from_slice(seed);
    off += seed.len();

    // owner
    buf[off..off + 32].copy_from_slice(&owner[..]);
    off += 32;

    // sha256(buf[..off]) -> 32 bytes
    let mut out = [0u8; 32];
    const SUCCESS: u64 = 0;
    let rc = unsafe { pinocchio::syscalls::sol_sha256(buf.as_ptr(), off as u64, out.as_mut_ptr()) };
    if rc != SUCCESS {
        return Err(ProgramError::InvalidInstructionData);
    }

    Ok(out)
}

pub fn process_authorize_checked_with_seed(
    accounts: &[AccountInfo],
    args: AuthorizeCheckedWithSeedData, // has: new_authorized, stake_authorize, authority_seed, authority_owner
) -> ProgramResult {
    // Native requires 4 accounts (1 sysvar):
    // stake, old_authority_base, clock, new_authority [, optional custodian, ...]
    if accounts.len() < 4 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    let [stake_ai, old_base_ai, clock_ai, new_auth_ai, rest @ ..] = accounts else {
        return Err(ProgramError::InvalidAccountData);
    };

    // Safety checks that match native intent
    if *stake_ai.owner() != crate::ID || !stake_ai.is_writable() {
        return Err(ProgramError::IncorrectProgramId);
    }
    if clock_ai.key() != &pinocchio::sysvars::clock::CLOCK_ID {
        return Err(ProgramError::InvalidArgument);
    }
    // New authority must be a signer (native uses collect_signers_checked(Some(new_auth), ...))
    if !new_auth_ai.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    // Do NOT require `old_base_ai` to be a signer. If it is, we’ll derive and add the seed key.
    // Do NOT pre-require optional custodian to sign here; authorize_update will enforce only if lockup is active.
    let maybe_lockup_authority: Option<&AccountInfo> = rest.first();

    // Load sysvar clock
    let clock = unsafe { Clock::from_account_info_unchecked(clock_ai)? };

    // Gather existing transaction signers (could include unrelated signers)
    let mut signers_buf = [Pubkey::default(); MAXIMUM_SIGNERS];
    let mut n = collect_signers(accounts, &mut signers_buf)?;

    // If old base signed, insert the derived seed authority into the signer set
    if old_base_ai.is_signer() {
        let derived = derive_with_seed_compat(old_base_ai.key(), &args.authority_seed, &args.authority_owner)?;
        if n >= MAXIMUM_SIGNERS {
            return Err(ProgramError::InvalidInstructionData);
        }
        signers_buf[n] = derived;
        n += 1;
    }

    let signers = &signers_buf[..n];

    // In *checked* variants, the new authority is the 4th account, not from args.
    let new_authorized: Pubkey = *new_auth_ai.key();

    // Load -> update -> store (mirrors native `do_authorize`)
    match get_stake_state(stake_ai)? {
        StakeStateV2::Initialized(mut meta) => {
            authorize_update(
                &mut meta,
                new_authorized,
                args.stake_authorize,
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
                args.stake_authorize,
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