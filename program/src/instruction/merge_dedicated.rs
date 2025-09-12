use crate::{
    helpers::{
        collect_signers,
        constant::MAXIMUM_SIGNERS,
        get_stake_state,
        relocate_lamports,
        set_stake_state,
    },
    state::{stake_state_v2::StakeStateV2, MergeKind, StakeHistorySysvar},
    ID,
};

use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::{clock::Clock, Sysvar},
    ProgramResult,
};

pub fn process_merge(accounts: &[AccountInfo]) -> ProgramResult {
    // native asserts: 4 accounts (2 sysvars)
    // [destination, source, clock, stake_history, ...optional...]
    let [dst_ai, src_ai, clock_ai, _stake_history_info, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    // basic checks
    if dst_ai.key() == src_ai.key() {
        return Err(ProgramError::InvalidArgument);
    }
    if *dst_ai.owner() != ID || *src_ai.owner() != ID {
        return Err(ProgramError::InvalidAccountOwner);
    }
    if !dst_ai.is_writable() || !src_ai.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }

    // load sysvars
    let clock = Clock::from_account_info(clock_ai)?;
    // Native uses the epoch wrapper; content of history acct is not read here.
    let stake_history = StakeHistorySysvar(clock.epoch);

    // collect signers (native collect_signers)
    let mut signer_buf = [Pubkey::default(); MAXIMUM_SIGNERS];
    let n = collect_signers(accounts, &mut signer_buf)?;
    let signers = &signer_buf[..n];

    // classify destination & require staker auth (native)
    let dst_state = get_stake_state(dst_ai)?;
    let dst_kind = MergeKind::get_if_mergeable(
        &dst_state,
        dst_ai.lamports(),
        &clock,
        &stake_history,
    )?;

    // Authorized staker is required to merge
    if !signers
        .iter()
        .any(|s| *s == dst_kind.meta().authorized.staker)
    {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // classify source
    let src_state = get_stake_state(src_ai)?;
    let src_kind = MergeKind::get_if_mergeable(
        &src_state,
        src_ai.lamports(),
        &clock,
        &stake_history,
    )?;

    // perform merge (native shape logic is inside MergeKind::merge)
    if let Some(merged_state) = dst_kind.merge(src_kind, &clock)? {
        set_stake_state(dst_ai, &merged_state)?;
    }

    // deinit & drain source (native)
    set_stake_state(src_ai, &StakeStateV2::Uninitialized)?;
    relocate_lamports(src_ai, dst_ai, src_ai.lamports())?;

    Ok(())
}