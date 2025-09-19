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
    sysvars::clock::Clock,
    ProgramResult,
};

pub fn process_merge(accounts: &[AccountInfo]) -> ProgramResult {
    // Expected accounts (4): [destination, source, clock, stake_history, ...optional...]
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

    // Load sysvars
    let clock = Clock::from_account_info(clock_ai)?;
    // Use the epoch wrapper; contents of history account are not read here
    let stake_history = StakeHistorySysvar(clock.epoch);

    // Collect signers
    let mut signer_buf = [Pubkey::default(); MAXIMUM_SIGNERS];
    let n = collect_signers(accounts, &mut signer_buf)?;
    let signers = &signer_buf[..n];

    // Classify destination & require staker auth
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

    // Classify source
    let src_state = get_stake_state(src_ai)?;
    let src_kind = MergeKind::get_if_mergeable(
        &src_state,
        src_ai.lamports(),
        &clock,
        &stake_history,
    )?;

    // Ensure metadata compatibility (authorities equal, lockups compatible)
    MergeKind::metas_can_merge(dst_kind.meta(), src_kind.meta(), &clock)?;

    // Perform merge
    if let Some(merged_state) = dst_kind.merge(src_kind, &clock)? {
        set_stake_state(dst_ai, &merged_state)?;
    }

    // Deinitialize and drain source
    set_stake_state(src_ai, &StakeStateV2::Uninitialized)?;
    relocate_lamports(src_ai, dst_ai, src_ai.lamports())?;

    Ok(())
}
