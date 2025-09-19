use pinocchio::{
    account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey, sysvars::clock::Clock,
    ProgramResult,
};

use crate::{
    helpers::{collect_signers, get_stake_state, set_stake_state, MAXIMUM_SIGNERS},
    state::{stake_state_v2::StakeStateV2, StakeAuthorize},
};
use crate::helpers::authorize_update; 

/*fn parse_authorize_data(data: &[u8]) -> Result<AuthorizeData, ProgramError> {
    if data.len() != 33 { return Err(ProgramError::InvalidInstructionData); }
    let new_authorized =
        Pubkey::try_from(&data[0..32]).map_err(|_| ProgramError::InvalidInstructionData)?;
    let stake_authorize = match data[32] {
        0 => StakeAuthorize::Staker,
        1 => StakeAuthorize::Withdrawer,
        _ => return Err(ProgramError::InvalidInstructionData),
    };
    Ok(AuthorizeData { new_authorized, stake_authorize })
}*/

pub fn process_authorize(
    accounts: &[AccountInfo],
    new_authority: Pubkey,
    authority_type: StakeAuthorize,
) -> ProgramResult { 
    if accounts.len() < 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let [stake_ai, clock_ai, rest @ ..] = accounts else {
        return Err(ProgramError::InvalidAccountData);
    };

    if *stake_ai.owner() != crate::ID || !stake_ai.is_writable() {
        return Err(ProgramError::IncorrectProgramId);
    }
    if clock_ai.key() != &pinocchio::sysvars::clock::CLOCK_ID {
        return Err(ProgramError::InvalidArgument);
    }
    let clock = unsafe { Clock::from_account_info_unchecked(clock_ai)? };

    // Optional lockup custodian (as a reference)
    let maybe_lockup_authority: Option<&AccountInfo> = rest.first();

    // Collect all signers
    let mut signers_buf = [Pubkey::default(); MAXIMUM_SIGNERS];// Stack allocated
    let n = collect_signers(accounts, &mut signers_buf)?;
    let signers = &signers_buf[..n];

    // Load, update, store
    match get_stake_state(stake_ai)? {
        StakeStateV2::Initialized(mut meta) => {
            authorize_update(
                &mut meta,
                new_authority,
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
                new_authority,
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
