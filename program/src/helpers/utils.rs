use crate::helpers::constant::*;
use crate::state::stake_state_v2::StakeStateV2;
use crate::ID;
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

// fn get_stake_state(stake_account_info: &AccountInfo) -> Result<StakeStateV2, ProgramError> {
//     if *stake_account_info.owner() != ID {
//         return Err(ProgramError::InvalidAccountOwner);
//     }
// }
