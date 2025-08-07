use crate::{helpers::*, state::stake_state_v2::StakeStateV2, state::StakeHistorySysvar};
use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::{clock::Clock, Sysvar},
    ProgramResult,
};

fn process_split(accounts: &[AccountInfo], split_lamports: u64) -> ProgramResult {
    let mut arr_of_signers = [Pubkey::default(); MAXIMUM_SIGNERS];
    let _ = collect_signers(accounts, &mut arr_of_signers)?;

    let [source_stake_account_info, destination_stake_account_info, _] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let clock = Clock::get()?;
    let stake_history = &StakeHistorySysvar(clock.epoch);

    let destination_data_len = destination_stake_account_info.data_len();
    if destination_data_len != StakeStateV2::size_of() {
        return Err(ProgramError::InvalidAccountData);
    }

    if let StakeStateV2::Uninitialized =
        StakeStateV2::get_stake_state(destination_stake_account_info)?
    {
    } else {
        return Err(ProgramError::InvalidAccountData);
    }

    let source_lamport_balance = source_stake_account_info.lamports();
    let destination_lamport_balance = destination_stake_account_info.lamports();

    if split_lamports > source_lamport_balance {
        return Err(ProgramError::InsufficientFunds);
    }
    Ok(())
}
