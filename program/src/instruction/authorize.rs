use pinocchio::{
    account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey, sysvars::clock::Clock,
    ProgramResult,
};

use crate::{
    helpers::{collect_signers, next_account_info, MAXIMUM_SIGNERS},
    state::{accounts::AuthorizeData, stake_state_v2::StakeStateV2, state::Meta, StakeAuthorize},
};

// [0..32] new_authorized pubkey | [32] role (0=Staker, 1=Withdrawer)
fn parse_authorize_data(data: &[u8]) -> Result<AuthorizeData, ProgramError> {
    if data.len() < 33 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let new_authorized =
        Pubkey::try_from(&data[0..32]).map_err(|_| ProgramError::InvalidInstructionData)?;
    let stake_authorize = match data[32] {
        0 => StakeAuthorize::Staker,
        1 => StakeAuthorize::Withdrawer,
        _ => return Err(ProgramError::InvalidInstructionData),
    };
    Ok(AuthorizeData {
        new_authorized,
        stake_authorize,
    })
}

pub fn process_authorize(accounts: &[AccountInfo], instruction_data: &[u8]) -> ProgramResult {
    let mut signers_buf = [Pubkey::default(); MAXIMUM_SIGNERS];
    let signers_len = collect_signers(accounts, &mut signers_buf)?;

    let [stake_account, clock_info, _current_authority, rest @ ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    let maybe_lockup_authority = rest.first();

    if clock_info.key() != &pinocchio::sysvars::clock::CLOCK_ID {
        return Err(ProgramError::InvalidArgument);
    }
    let clock = unsafe { Clock::from_account_info_unchecked(clock_info)? };

    let authorize_data = parse_authorize_data(instruction_data)?;

    // Safety checks on stake account
    if stake_account.owner() != &crate::ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !stake_account.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }

    // Construct set of signer pubkeys
    let signers = &signers_buf[..signers_len];

    // Deserialize stake state and prepare to write back
    let data_ref = unsafe { stake_account.borrow_mut_data_unchecked() };
    let mut stake_state = StakeStateV2::deserialize(data_ref)?;

    let signer_contains = |key: &Pubkey| signers.iter().any(|s| s == key);

    match &mut stake_state {
        StakeStateV2::Initialized(meta) => {
            apply_authorize(
                meta,
                &authorize_data,
                maybe_lockup_authority,
                &clock,
                signer_contains,
            )?;
        }
        StakeStateV2::Stake(meta, _stake, _flags) => {
            apply_authorize(
                meta,
                &authorize_data,
                maybe_lockup_authority,
                &clock,
                signer_contains,
            )?;
        }
        _ => return Err(ProgramError::InvalidAccountData),
    }

    // Re-serialize into account data
    StakeStateV2::serialize(&stake_state, data_ref)?;

    Ok(())
}

fn apply_authorize(
    meta: &mut Meta,
    authorize_data: &AuthorizeData,
    maybe_lockup_authority: Option<&AccountInfo>,
    clock: &Clock,
    signer_contains: impl Fn(&Pubkey) -> bool,
) -> ProgramResult {
    match authorize_data.stake_authorize {
        StakeAuthorize::Staker => {
            // staker change requires current staker signature
            if !signer_contains(&meta.authorized.staker) {
                return Err(ProgramError::MissingRequiredSignature);
            }
            meta.authorized.staker = authorize_data.new_authorized;
        }
        StakeAuthorize::Withdrawer => {
            // withdrawer change requires current withdrawer signature
            if !signer_contains(&meta.authorized.withdrawer) {
                return Err(ProgramError::MissingRequiredSignature);
            }

            // Enforce lockup: if lockup active, require custodian signature
            let lockup_active = meta.lockup.unix_timestamp > clock.unix_timestamp
                || meta.lockup.epoch > clock.epoch;
            if lockup_active {
                if let Some(lockup_auth) = maybe_lockup_authority {
                    if !(lockup_auth.is_signer() && lockup_auth.key() == &meta.lockup.custodian) {
                        return Err(ProgramError::MissingRequiredSignature);
                    }
                } else {
                    return Err(ProgramError::MissingRequiredSignature);
                }
            }

            meta.authorized.withdrawer = authorize_data.new_authorized;
        }
    }

    Ok(())
}
