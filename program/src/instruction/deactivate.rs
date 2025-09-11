use pinocchio::{
    account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey, sysvars::clock::Clock,
    ProgramResult,
};

// Imports from crate
use crate::{
    helpers::{bytes_to_u64, collect_signers, MAXIMUM_SIGNERS},
    id,
    state::stake_state_v2::StakeStateV2,
};

pub fn process_deactivate(accounts: &[AccountInfo]) -> ProgramResult {
    // Validate account inputs (minimum 2 accounts: stake, clock; optional staker and lockup authority)
    if accounts.len() < 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    let [stake_account, clock_info, rest @ ..] = accounts else {
        return Err(ProgramError::InvalidAccountData);
    };

    // Optional staker and lockup authority
    let staker = if rest.is_empty() {
        return Err(ProgramError::NotEnoughAccountKeys);
    } else {
        &rest[0]
    };
    let maybe_lockup_authority = if rest.len() > 1 { Some(&rest[1]) } else { None };

    // Safety checks
    if !stake_account.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }
    if stake_account.owner() != &id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !staker.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if clock_info.key() != &pinocchio::sysvars::clock::CLOCK_ID {
        return Err(ProgramError::InvalidArgument);
    }
    if let Some(lockup_auth) = maybe_lockup_authority {
        if !lockup_auth.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }
    }

    // Load clock sysvar
    let clock = unsafe { Clock::from_account_info_unchecked(clock_info)? };

    // Collect signers
    let mut signers_buf = [Pubkey::default(); MAXIMUM_SIGNERS];
    let signers_len = collect_signers(accounts, &mut signers_buf)?;
    let signers = &signers_buf[..signers_len];
    let signer_contains = |key: &Pubkey| signers.iter().any(|s| s == key);

    // Deserialize stake state
    let data_ref = unsafe { stake_account.borrow_mut_data_unchecked() };
    let mut stake_state = StakeStateV2::deserialize(data_ref)?;

    // Process deactivation
    match &mut stake_state {
        StakeStateV2::Stake(meta, stake, _stake_flags) => {
            // Check if the staker is authorized
            if !signer_contains(&meta.authorized.staker) {
                return Err(ProgramError::Custom(1)); // Custom error for InvalidAuthority
            }

            // Check lockup constraints
            if meta.lockup.is_active(clock.unix_timestamp, clock.epoch) {
                let custodian_authorized =
                    maybe_lockup_authority.map_or(false, |c| c.key() == &meta.lockup.custodian);
                if !custodian_authorized {
                    return Err(ProgramError::Custom(2)); // Custom error for LockupInForce
                }
            }

            // Check if stake is fully activated
            if !stake.delegation.is_fully_activated(clock.epoch) {
                return Err(ProgramError::Custom(3)); // Custom error for InvalidStakeState
            }

            // Check if already deactivated
            if bytes_to_u64(stake.delegation.deactivation_epoch) != u64::MAX {
                return Err(ProgramError::Custom(4)); // Custom error for AlreadyDeactivated
            }

            // Set deactivation epoch
            stake.delegation.deactivation_epoch = clock.epoch.to_le_bytes();
        }
        _ => return Err(ProgramError::InvalidAccountData),
    }

    // Serialize updated stake state
    StakeStateV2::serialize(&stake_state, data_ref)?;

    Ok(())
}
