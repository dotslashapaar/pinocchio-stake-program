// use pinocchio::{
//     account_info::AccountInfo,
//     instruction::{Seed, Signer},
//     program_error::ProgramError,
//     pubkey::{self, Pubkey},
//     sysvars::rent::Rent,
//     ProgramResult,
// };


// use solana_program::stake::state::Authorized;


// fn initialize(
//         accounts: &[AccountInfo],
//         authorized: Authorized,
//         lockup: Lockup,
//     ) -> ProgramResult {
//         let account_info_iter = &mut accounts.iter();

//         // native asserts: 2 accounts (1 sysvar)
//         let stake_account_info = next_account_info(account_info_iter)?;
//         let rent_info = next_account_info(account_info_iter)?;

//         let rent = &Rent::from_account_info(rent_info)?;

//         // `get_stake_state()` is called unconditionally, which checks owner
//         do_initialize(stake_account_info, authorized, lockup, rent)?;

//         Ok(())
//     }