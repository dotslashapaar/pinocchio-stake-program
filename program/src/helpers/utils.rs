use crate::helpers::constant::*;
use crate::state::stake_history::StakeHistorySysvar;
use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvars::{rent::Rent, Sysvar},
    ProgramResult,
};

use crate::error::{to_program_error, StakeError};
use crate::state::stake_state_v2::StakeStateV2;
// NOTE: adjust this import to where your VoteState actually lives.
// If your vote_state.rs is at crate root: use crate::vote_state::VoteState;
// If it’s under state::vote_state: use crate::state::vote_state::VoteState;
use crate::state::vote_state::VoteState;

// Pull the stake structs from where you re-exported them.
// If you have `pub use accounts::*;` in state/mod.rs, this works:
use crate::state::{Delegation, Meta, Stake};
use crate::ID;

const FEATURE_STAKE_RAISE_MINIMUM_DELEGATION_TO_1_SOL: bool = false;
const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

pub struct ValidatedDelegatedInfo {
    pub stake_amount: u64,
}

pub enum ErrorCode {
    TOOMANYSIGNERS = 0x1,
}

pub fn collect_signers(
    accounts: &[AccountInfo],
    array_of_signers: &mut [Pubkey; MAXIMUM_SIGNERS],
) -> Result<usize, ProgramError> {
    let mut len_of_signers = 0;

    for account in accounts {
        if account.is_signer() {
            if len_of_signers < MAXIMUM_SIGNERS {
                array_of_signers[len_of_signers] = *account.key();
                len_of_signers += 1;
            } else {
                return Err(ProgramError::Custom(ErrorCode::TOOMANYSIGNERS as u32));
            }
        }
    }

    Ok(len_of_signers)
}

pub fn next_account_info<'a, I: Iterator<Item = &'a AccountInfo>>(
    iter: &mut I,
) -> Result<&'a AccountInfo, ProgramError> {
    iter.next().ok_or(ProgramError::NotEnoughAccountKeys)
}

#[inline(always)]
pub fn get_minimum_delegation() -> u64 {
    if FEATURE_STAKE_RAISE_MINIMUM_DELEGATION_TO_1_SOL {
        const MINIMUM_DELEGATION_SOL: u64 = 1;
        MINIMUM_DELEGATION_SOL * LAMPORTS_PER_SOL
    } else {
        1
    }
}

// Use numeric compare (convert [u8;8] to u64) — DO NOT compare byte arrays directly.
pub fn warmup_cooldown_rate(
    current_epoch_le: [u8; 8],
    new_rate_activation_epoch_le: Option<[u8; 8]>,
) -> f64 {
    let current_epoch = u64::from_le_bytes(current_epoch_le);
    let activation_at = new_rate_activation_epoch_le
        .map(u64::from_le_bytes)
        .unwrap_or(u64::MAX);

    if current_epoch < activation_at {
        DEFAULT_WARMUP_COOLDOWN_RATE
    } else {
        NEW_WARMUP_COOLDOWN_RATE
    }
}

pub type Epoch = [u8; 8];

#[inline]
pub fn bytes_to_u64(bytes: [u8; 8]) -> u64 {
    u64::from_le_bytes(bytes)
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct ValidatedSplitInfo {
    pub source_remaining_balance: u64,
    pub destination_rent_exempt_reserve: u64,
}

pub(crate) fn validate_split_amount(
    source_lamports: u64,
    destination_lamports: u64,
    split_lamports: u64,
    source_meta: &Meta,
    destination_data_len: usize,
    additional_required_lamports: u64,
    source_is_active: bool,
) -> Result<ValidatedSplitInfo, ProgramError> {
    if split_lamports == 0 {
        return Err(ProgramError::InsufficientFunds);
    }
    if split_lamports > source_lamports {
        return Err(ProgramError::InsufficientFunds);
    }

    let source_minimum_balance =
        bytes_to_u64(source_meta.rent_exempt_reserve).saturating_add(additional_required_lamports);
    let source_remaining_balance = source_lamports.saturating_sub(split_lamports);

    if source_remaining_balance != 0 && source_remaining_balance < source_minimum_balance {
        return Err(ProgramError::InsufficientFunds);
    }

    let rent = Rent::get()?;
    let destination_rent_exempt_reserve = rent.minimum_balance(destination_data_len);

    if source_is_active
        && source_remaining_balance != 0
        && destination_lamports < destination_rent_exempt_reserve
    {
        return Err(ProgramError::InsufficientFunds);
    }

    let destination_minimum_balance =
        destination_rent_exempt_reserve.saturating_add(additional_required_lamports);
    let destination_balance_deficit =
        destination_minimum_balance.saturating_sub(destination_lamports);
    if split_lamports < destination_balance_deficit {
        return Err(ProgramError::InsufficientFunds);
    }

    Ok(ValidatedSplitInfo {
        source_remaining_balance,
        destination_rent_exempt_reserve,
    })
}

// Return a deserialized vote state (zero-copy read; adjust if you changed VoteState).
pub fn get_vote_state(vote_account_info: &AccountInfo) -> Result<VoteState, ProgramError> {
    let data = unsafe { vote_account_info.borrow_data_unchecked() };
    if data.len() < core::mem::size_of::<VoteState>() {
        return Err(ProgramError::InvalidAccountData);
    }
    let vote_state = unsafe { &*(data.as_ptr() as *const VoteState) };
    Ok(vote_state.clone())
}

// Load stake state from account via manual deserialize
pub fn get_stake_state(stake_account_info: &AccountInfo) -> Result<StakeStateV2, ProgramError> {
    if *stake_account_info.owner() != ID {
        return Err(ProgramError::InvalidAccountOwner);
    }
    let data = unsafe { stake_account_info.borrow_data_unchecked() };
    StakeStateV2::deserialize(&data)
}

// Write stake state back into account via manual serialize
pub fn set_stake_state(
    stake_account_info: &AccountInfo,
    stake_state: &StakeStateV2,
) -> Result<(), ProgramError> {
    let mut data = unsafe { stake_account_info.borrow_mut_data_unchecked() };
    stake_state.serialize(&mut data)?;
    Ok(())
}

// compute stake amount = lamports - rent exempt reserve
pub fn validate_delegated_amount(
    stake_account_info: &AccountInfo,
    meta: &Meta,
) -> Result<ValidatedDelegatedInfo, ProgramError> {
    let stake_amount = stake_account_info
        .lamports()
        .checked_sub(bytes_to_u64(meta.rent_exempt_reserve))
        .ok_or(StakeError::InsufficientFunds)
        .map_err(to_program_error)?;

    Ok(ValidatedDelegatedInfo { stake_amount })
}

// Create a new Stake from inputs (using your concrete field types)
pub fn new_stake(
    stake_amount: u64,
    vote_pubkey: &Pubkey,
    vote_state: &VoteState,
    activation_epoch: u64,
) -> Stake {
    let delegation = Delegation {
        voter_pubkey: *vote_pubkey,
        stake: stake_amount,
        activation_epoch,
        deactivation_epoch: u64::MAX,
        warmup_cooldown_rate: DEFAULT_WARMUP_COOLDOWN_RATE,
    };
    Stake {
        delegation,
        credits_observed: vote_state.credits(),
    }
}

// Modify an existing Stake’s delegation safely
pub fn redelegate_stake(
    stake: &mut Stake,
    stake_amount: u64,
    vote_pubkey: &Pubkey,
    vote_state: &VoteState,
    clock_epoch: u64,
    _stake_history: &StakeHistorySysvar,
) -> Result<(), ProgramError> {
    stake.delegation.voter_pubkey = *vote_pubkey;
    stake.delegation.stake = stake_amount;
    stake.delegation.activation_epoch = clock_epoch;
    stake.credits_observed = vote_state.credits();
    Ok(())
}

// Move lamports between two accounts (checked)
pub fn relocate_lamports(
    source_account_info: &AccountInfo,
    destination_account_info: &AccountInfo,
    lamports: u64,
) -> ProgramResult {
    {
        let mut source_lamports = source_account_info.try_borrow_mut_lamports()?;
        *source_lamports = source_lamports
            .checked_sub(lamports)
            .ok_or(ProgramError::InsufficientFunds)?;
    }
    {
        let mut destination_lamports = destination_account_info.try_borrow_mut_lamports()?;
        *destination_lamports = destination_lamports
            .checked_add(lamports)
            .ok_or(ProgramError::ArithmeticOverflow)?;
    }
    Ok(())
}

const SUCCESS: u64 = 0;

pub fn get_sysvar(
    dst: &mut [u8],
    sysvar_id: &Pubkey,
    offset: u64,
    length: u64,
) -> Result<(), ProgramError> {
    if dst.len() < length as usize {
        return Err(ProgramError::InvalidArgument);
    }
    let sysvar_id = sysvar_id as *const _ as *const u8;
    let var_addr = dst as *mut _ as *mut u8;

    #[cfg(feature = "solana")]
    let result =
        unsafe { pinocchio::syscalls::sol_get_sysvar(sysvar_id, var_addr, offset, length) };

    #[cfg(not(feature = "solana"))]
    let result =
        unsafe { pinocchio::syscalls::sol_get_sysvar(sysvar_id, var_addr, offset, length) };

    match result {
        SUCCESS => Ok(()),
        e => Err(e.into()),
    }
}

#[inline]
pub(crate) fn checked_add(a: u64, b: u64) -> Result<u64, ProgramError> {
    a.checked_add(b).ok_or(ProgramError::InsufficientFunds)
}
