use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::clock::Clock,
    ProgramResult,
};

use crate::{
    error::to_program_error,
    helpers::{collect_signers, next_account_info},
    helpers::utils::{
        get_stake_state, get_vote_state, new_stake, redelegate_stake, set_stake_state,
        validate_delegated_amount, ValidatedDelegatedInfo,
    },
    helpers::constant::MAXIMUM_SIGNERS,
    state::{StakeAuthorize, StakeFlags, StakeHistorySysvar, StakeStateV2},
};

/// Redelegate/Delegate helper (works for initial delegation and redelegation)
pub fn redelegate(accounts: &[AccountInfo]) -> ProgramResult {
    // Collect signers from the full account list
    let mut signers_buf = [Pubkey::default(); MAXIMUM_SIGNERS];
    let n = collect_signers(accounts, &mut signers_buf)?;
    let signers = &signers_buf[..n];

    // Expected accounts: 5 (2 sysvars + stake config)
    let account_info_iter = &mut accounts.iter();
    let stake_account_info = next_account_info(account_info_iter)?;
    let vote_account_info  = next_account_info(account_info_iter)?;
    let clock_info         = next_account_info(account_info_iter)?;
    let _stake_history     = next_account_info(account_info_iter)?; // present but not read directly
    let _stake_config      = next_account_info(account_info_iter)?; // present but not read directly

    let clock = &Clock::from_account_info(clock_info)?;
    let stake_history = StakeHistorySysvar(clock.epoch);

    let vote_state = get_vote_state(vote_account_info)?;

    match get_stake_state(stake_account_info)? {
        StakeStateV2::Initialized(meta) => {
            // staker must sign
            meta.authorized
                .check(signers, StakeAuthorize::Staker)
                .map_err(to_program_error)?;

            // how much can be delegated (lamports - rent)
            let ValidatedDelegatedInfo { stake_amount } =
                validate_delegated_amount(stake_account_info, &meta)?;

            // create stake delegated to the vote account
            let stake = new_stake(
                stake_amount,
                vote_account_info.key(),
                &vote_state,
                clock.epoch,
            );

            set_stake_state(
                stake_account_info,
                &StakeStateV2::Stake(meta, stake, StakeFlags::empty()),
            )?;
        }
        StakeStateV2::Stake(meta, mut stake, flags) => {
            // staker must sign
            meta.authorized
                .check(signers, StakeAuthorize::Staker)
                .map_err(to_program_error)?;

            let ValidatedDelegatedInfo { stake_amount } =
                validate_delegated_amount(stake_account_info, &meta)?;

            // Delegate helper enforces the active-stake rules & rescind-on-same-voter case.
            redelegate_stake(
                &mut stake,
                stake_amount,
                vote_account_info.key(),
                &vote_state,
                clock.epoch,
                &stake_history,
            )?;

            set_stake_state(stake_account_info, &StakeStateV2::Stake(meta, stake, flags))?;
        }
        _ => return Err(ProgramError::InvalidAccountData),
    }

    Ok(())
}
