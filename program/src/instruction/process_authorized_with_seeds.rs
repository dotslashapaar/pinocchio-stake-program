use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::clock::Clock,
    ProgramResult,
};

use crate::{
    helpers::{collect_signers, get_stake_state, set_stake_state, MAXIMUM_SIGNERS},
    helpers::authorize_update,
    state::{
        accounts::AuthorizeWithSeedData,
        stake_state_v2::StakeStateV2,
        StakeAuthorize,
    },
};


// Definition: sha256(base || seed || owner)
fn derive_with_seed_compat(
    base: &Pubkey,
    seed: &[u8],
    owner: &Pubkey,
) -> Result<Pubkey, ProgramError> {
    // Enforce max seed length 32 bytes for `create_with_seed`
    if seed.len() > 32 {
        return Err(ProgramError::InvalidInstructionData);
    }

    // Concatenate base || seed || owner into a fixed buffer (max 96 bytes)
    let mut buf = [0u8; 32 + 32 + 32];
    let mut off = 0usize;

    // base (32)
    buf[off..off + 32].copy_from_slice(&base[..]);
    off += 32;

    // seed (<= 32)
    buf[off..off + seed.len()].copy_from_slice(seed);
    off += seed.len();

    // owner (32)
    buf[off..off + 32].copy_from_slice(&owner[..]);
    off += 32;

    // sha256(buf[..off]) -> 32 bytes
    let mut out = [0u8; 32];
    // Call syscall directly
    let rc = unsafe {
        pinocchio::syscalls::sol_sha256(buf.as_ptr(), off as u64, out.as_mut_ptr())
    };
    const SUCCESS: u64 = 0;
    if rc != SUCCESS {
        return Err(ProgramError::InvalidInstructionData);
    }

    Ok(out)
}

pub fn process_authorized_with_seeds(
    accounts: &[AccountInfo],
    args: AuthorizeWithSeedData, // already has: new_authorized, stake_authorize, authority_seed, authority_owner
) -> ProgramResult { 
    // Required accounts: stake, base, clock (optional custodian)
    if accounts.len() < 3 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    // stake, base, clock, [maybe custodian, ...]
    let [stake_ai, base_ai, clock_ai, rest @ ..] = accounts else {
        return Err(ProgramError::InvalidAccountData);
    };

    // Basic safety checks
    if *stake_ai.owner() != crate::ID || !stake_ai.is_writable() {
        return Err(ProgramError::IncorrectProgramId);
    }
    if clock_ai.key() != &pinocchio::sysvars::clock::CLOCK_ID {
        return Err(ProgramError::InvalidArgument);
    }

    // Load clock
    let clock = unsafe { Clock::from_account_info_unchecked(clock_ai)? };

    // Optional lockup custodian account (pass-through to policy)
    let maybe_lockup_authority: Option<&AccountInfo> = rest.first();

   
    // Build the signer set
    let mut signers_buf = [Pubkey::default(); MAXIMUM_SIGNERS];
    let mut n = collect_signers(accounts, &mut signers_buf)?;
    let mut push_signer = |pk: Pubkey| -> Result<(), ProgramError> {
        if n >= MAXIMUM_SIGNERS {
            return Err(ProgramError::InvalidInstructionData);
        }
        signers_buf[n] = pk;
        n += 1;
        Ok(())
    };

    // If the base signed, no additional derivation is needed; the base signature suffices

    // Final signer slice we pass to the policy
    let signers = &signers_buf[..n];

    // Load state, apply policy update, write back
    match get_stake_state(stake_ai)? {
        StakeStateV2::Initialized(mut meta) => {
            authorize_update(
                &mut meta,
                args.new_authorized,
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
                args.new_authorized,
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
