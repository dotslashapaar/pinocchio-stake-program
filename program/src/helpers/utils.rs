use crate::helpers::constant::*;
use pinocchio::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};
pub enum ErrorCode {
    TOOMANYSIGNERS = 0x1,
}

// almost all native stake program processors accumulate every account signer
// they then defer all signer validation to functions on Meta or Authorized
// this results in an instruction interface that is much looser than the one documented
// to avoid breaking backwards compatibility, we do the same here
// in the future, we may decide to tighten the interface and break badly formed transactions
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

/// The minimum stake amount that can be delegated, in lamports.
/// NOTE: This is also used to calculate the minimum balance of a delegated
/// stake account, which is the rent exempt reserve _plus_ the minimum stake
/// delegation.
#[inline(always)]
pub fn get_minimum_delegation() -> u64 {
    if FEATURE_STAKE_RAISE_MINIMUM_DELEGATION_TO_1_SOL {
        const MINIMUM_DELEGATION_SOL: u64 = 1;
        MINIMUM_DELEGATION_SOL * LAMPORTS_PER_SOL
    } else {
        1
    }
}
pub fn warmup_cooldown_rate(
    current_epoch: [u8; 8],
    new_rate_activation_epoch: Option<[u8; 8]>,
) -> f64 {
    if current_epoch < new_rate_activation_epoch.unwrap_or(u64::MAX.to_le_bytes()) {
        DEFAULT_WARMUP_COOLDOWN_RATE
    } else {
        NEW_WARMUP_COOLDOWN_RATE
    }
}

pub type Epoch = [u8; 8];

pub fn bytes_to_u64(bytes: [u8; 8]) -> u64 {
    u64::from_le_bytes(bytes)
}
