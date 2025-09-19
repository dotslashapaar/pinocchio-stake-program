// Delegate instruction
use pinocchio::{
    account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey, sysvars::clock::Clock,
    ProgramResult,
};

use crate::error::to_program_error;
use crate::helpers::{
    collect_signers, next_account_info, MAXIMUM_SIGNERS, validate_delegated_amount,
    ValidatedDelegatedInfo,
};
use crate::helpers::utils::{
    get_stake_state, get_vote_credits, new_stake_with_credits, redelegate_stake_with_credits,
    set_stake_state,
};
use crate::state::stake_history::StakeHistorySysvar;
use crate::state::{StakeAuthorize, StakeFlags, StakeStateV2};

pub fn process_delegate(accounts: &[AccountInfo]) -> ProgramResult {
    // Gather signers
    let mut signers_array = [Pubkey::default(); MAXIMUM_SIGNERS];
    let signers_count = collect_signers(accounts, &mut signers_array)?;
    let signers = &signers_array[..signers_count];

    // Expected accounts: stake, vote, clock, stake_history, stake_config
    let account_info_iter = &mut accounts.iter();
    let stake_account_info = next_account_info(account_info_iter)?;
    let vote_account_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    let _stake_history_info = next_account_info(account_info_iter)?;
    let _stake_config_info = next_account_info(account_info_iter)?;

    let clock = &Clock::from_account_info(clock_info)?;
    let stake_history = &StakeHistorySysvar(clock.epoch);

    let vote_credits = get_vote_credits(vote_account_info)?;

    match get_stake_state(stake_account_info)? {
        StakeStateV2::Initialized(meta) => {
            // Staker must sign
            meta.authorized
                .check(signers, StakeAuthorize::Staker)
                .map_err(to_program_error)?;

            // Amount delegated = lamports - rent_exempt_reserve
            let ValidatedDelegatedInfo { stake_amount } =
                validate_delegated_amount(stake_account_info, &meta)?;

            // Create stake and store
            let stake = new_stake_with_credits(
                stake_amount,
                vote_account_info.key(),
                clock.epoch,
                vote_credits,
            );

            set_stake_state(
                stake_account_info,
                &StakeStateV2::Stake(meta, stake, StakeFlags::empty()),
            )
        }
        StakeStateV2::Stake(meta, mut stake, flags) => {
            // Staker must sign
            meta.authorized
                .check(signers, StakeAuthorize::Staker)
                .map_err(to_program_error)?;

            let ValidatedDelegatedInfo { stake_amount } =
                validate_delegated_amount(stake_account_info, &meta)?;

            // If deactivation is scheduled and target vote differs, reject (TooSoon)
            // Pre-check: if deactivating, only allow redelegation to the same vote
            let current_voter = stake.delegation.voter_pubkey;
            let deact_epoch = crate::helpers::bytes_to_u64(stake.delegation.deactivation_epoch);
            if deact_epoch != u64::MAX && current_voter != *vote_account_info.key() {
                return Err(to_program_error(crate::error::StakeError::TooSoonToRedelegate));
            }

            // Let helper update stake state (possible rescind or re-delegate)
            redelegate_stake_with_credits(
                &mut stake,
                stake_amount,
                vote_account_info.key(),
                vote_credits,
                clock.epoch,
                stake_history,
            )?;

            set_stake_state(stake_account_info, &StakeStateV2::Stake(meta, stake, flags))
        }
        _ => Err(ProgramError::InvalidAccountData),
    }?;

    Ok(())
}
