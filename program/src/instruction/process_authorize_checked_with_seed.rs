use pinocchio::{
    account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey, sysvars::clock::Clock,
    ProgramResult,
};

use crate::{
    helpers::{collect_signers, MAXIMUM_SIGNERS},
    id,
    state::{
        accounts::{AuthorizeCheckedWithSeedData, AuthorizeData},
        stake_state_v2::StakeStateV2,
    },
};

pub fn process_authorize_checked_with_seed(
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    // Validate account inputs (minimum 4 accounts: stake, base authority, clock, new authority)
    if accounts.len() < 4 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    let [stake_account, old_authority_base, clock_info, new_authority, rest @ ..] = accounts else {
        return Err(ProgramError::InvalidAccountData);
    };

    // Optional lockup authority
    let maybe_lockup_authority = rest.first();

    // Safety checks
    if !stake_account.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }
    if stake_account.owner() != &id() {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !new_authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !old_authority_base.is_signer() {
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

    // Parse instruction data
    let authorize_data = AuthorizeCheckedWithSeedData::parse(instruction_data)?;

    // Derive the expected old authority key from seed
    let seed = core::str::from_utf8(authorize_data.authority_seed)
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let derived_key = Pubkey::create_with_seed(
        old_authority_base.key(),
        seed,
        &authorize_data.authority_owner,
    )?;

    // Collect signers
    let mut signers_buf = [Pubkey::default(); MAXIMUM_SIGNERS];
    let signers_len = collect_signers(accounts, &mut signers_buf)?;
    let mut signers = signers_buf[..signers_len].to_vec();
    signers.push(derived_key); // Add derived key to signers

    // Define signer check closure
    let signer_contains = |key: &Pubkey| signers.iter().any(|s| s == key);

    // Deserialize stake state
    let data_ref = unsafe { stake_account.borrow_mut_data_unchecked() };
    let mut stake_state = StakeStateV2::deserialize(data_ref)?;

    // Apply authorization based on stake state
    match &mut stake_state {
        StakeStateV2::Initialized(meta) => {
            crate::instruction::authorize::apply_authorize(
                meta,
                &AuthorizeData {
                    new_authorized: authorize_data.new_authorized,
                    stake_authorize: authorize_data.stake_authorize,
                },
                maybe_lockup_authority,
                &clock,
                signer_contains,
            )?;
        }
        StakeStateV2::Stake(meta, _, _) => {
            crate::instruction::authorize::apply_authorize(
                meta,
                &AuthorizeData {
                    new_authorized: authorize_data.new_authorized,
                    stake_authorize: authorize_data.stake_authorize,
                },
                maybe_lockup_authority,
                &clock,
                signer_contains,
            )?;
        }
        _ => return Err(ProgramError::InvalidAccountData),
    }

    // Serialize updated stake state
    StakeStateV2::serialize(&stake_state, data_ref)?;

    Ok(())
}
