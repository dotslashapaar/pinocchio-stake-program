use crate::{
    helpers::get_minimum_delegation, instruction::{self, StakeInstruction}, state::{
        accounts::{
            AuthorizeCheckedWithSeedData, AuthorizeWithSeedData
        }, StakeAuthorize
    }
};
use pinocchio::{
    account_info::AccountInfo,
    default_panic_handler, msg, no_allocator, program_entrypoint,
    program_error::ProgramError, pubkey::Pubkey, ProgramResult,
};

// Program entrypoint boilerplate
program_entrypoint!(process_instruction);
no_allocator!();
default_panic_handler!();

#[inline(always)]
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    // Strip off the 1-byte discriminator; the remainder belongs to the variant
    let (disc, payload) = instruction_data
        .split_first()
        .ok_or(ProgramError::InvalidInstructionData)?;

    // Small helper for u64 payloads (lamports, etc.)
    let read_u64 = |data: &[u8]| -> Result<u64, ProgramError> {
        if data.len() != 8 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(data);
        Ok(u64::from_le_bytes(buf))
    };

    match StakeInstruction::try_from(disc)? {
        // --------------------------------------------------------------------
        // Initialization
        // --------------------------------------------------------------------
        StakeInstruction::Initialize => {
            msg!("Instruction: Initialize");
            if payload.len() != 112 {
    return Err(ProgramError::InvalidInstructionData);
}
let staker = Pubkey::try_from(&payload[0..32])
    .map_err(|_| ProgramError::InvalidInstructionData)?;
let withdrawer = Pubkey::try_from(&payload[32..64])
    .map_err(|_| ProgramError::InvalidInstructionData)?;
let unix_ts = i64::from_le_bytes(payload[64..72].try_into().unwrap());
let epoch   = u64::from_le_bytes(payload[72..80].try_into().unwrap());
let custodian = Pubkey::try_from(&payload[80..112])
    .map_err(|_| ProgramError::InvalidInstructionData)?;

let authorized = crate::state::accounts::Authorized { staker, withdrawer };
let lockup = crate::state::state::Lockup { unix_timestamp: unix_ts, epoch, custodian };

instruction::initialize::initialize(accounts, authorized, lockup)
        }
        StakeInstruction::InitializeChecked => {
            msg!("Instruction: InitializeChecked");
            // No payload; authorities are passed as accounts.
            instruction::initialize_checked::process_initialize_checked(accounts)
        }

        // --------------------------------------------------------------------
        // Authorization (4 variants)
        // --------------------------------------------------------------------
        StakeInstruction::Authorize => {
            msg!("Instruction: Authorize");
            // Expect 33 bytes: [0..32]=new pubkey, [32]=role
            if payload.len() != 33 {
                return Err(ProgramError::InvalidInstructionData);
            }
            let new_authority = Pubkey::try_from(&payload[..32])
                .map_err(|_| ProgramError::InvalidInstructionData)?;
            let authority_type = match payload[32] {
                0 => StakeAuthorize::Staker,
                1 => StakeAuthorize::Withdrawer,
                _ => return Err(ProgramError::InvalidInstructionData),
            };
            instruction::authorize::process_authorize(accounts, new_authority, authority_type)
        }

        StakeInstruction::AuthorizeWithSeed => {
            msg!("Instruction: AuthorizeWithSeed");
            let args = AuthorizeWithSeedData::parse(payload)?;
            // module path may be `authorize_with_seed` in your tree
            instruction::process_authorized_with_seeds::process_authorized_with_seeds(accounts, args)
        }

        StakeInstruction::AuthorizeChecked => {
            msg!("Instruction: AuthorizeChecked");
            // Expect exactly 1 byte: 0=Staker, 1=Withdrawer
            if payload.len() != 1 {
                return Err(ProgramError::InvalidInstructionData);
            }
            let authority_type = match payload[0] {
                0 => StakeAuthorize::Staker,
                1 => StakeAuthorize::Withdrawer,
                _ => return Err(ProgramError::InvalidInstructionData),
            };
            instruction::authorize_checked::process_authorize_checked(accounts, authority_type)
        }

        StakeInstruction::AuthorizeCheckedWithSeed => {
            msg!("Instruction: AuthorizeCheckedWithSeed");
            let args = AuthorizeCheckedWithSeedData::parse(payload)?;
            instruction::process_authorize_checked_with_seed::process_authorize_checked_with_seed(
                accounts,
                args,
            )
        }

        // --------------------------------------------------------------------
        // Stake lifecycle
        // --------------------------------------------------------------------
        StakeInstruction::DelegateStake => {
            msg!("Instruction: DelegateStake");
            // No payload; behavior mirrors native (stake, vote, clock, history, config, auth, …)
            instruction::process_delegate::process_delegate(accounts)
        }

        StakeInstruction::Split => {
            msg!("Instruction: Split");
            // Native Split carries the lamports to split
            let lamports = read_u64(payload)?;
            instruction::split::process_split(accounts, lamports)
        }

        StakeInstruction::Withdraw => {
            msg!("Instruction: Withdraw");
            let lamports = read_u64(payload)?;
            instruction::withdraw::process_withdraw(accounts, lamports)
        }

        StakeInstruction::Deactivate => {
            msg!("Instruction: Deactivate");
            instruction::deactivate::process_deactivate(accounts)
        }

        // --------------------------------------------------------------------
        // Lockup (2 variants)
        // --------------------------------------------------------------------
        StakeInstruction::SetLockup => {
            msg!("Instruction: SetLockup");
            // Payload carries lockup args; handler parses internally
            instruction::process_set_lockup::process_set_lockup(accounts, payload)
        }

        StakeInstruction::SetLockupChecked => {
            msg!("Instruction: SetLockupChecked");
            instruction::process_set_lockup_checked::process_set_lockup_checked(accounts, payload)
        }

        // --------------------------------------------------------------------
        // Merge
        // --------------------------------------------------------------------
        StakeInstruction::Merge => {
            msg!("Instruction: Merge");
            // No payload
            // If your file is named `merge_dedicated.rs`, use that module path:
            instruction::merge_dedicated::process_merge(accounts)
        }

        // --------------------------------------------------------------------
        // Move stake/lamports (post feature-activation)
        // --------------------------------------------------------------------
        StakeInstruction::MoveStake => {
            msg!("Instruction: MoveStake");
            let lamports = read_u64(payload)?;
            instruction::process_move_stake::process_move_stake(accounts, lamports)
        }
        StakeInstruction::MoveLamports => {
            msg!("Instruction: MoveLamports");
            let lamports = read_u64(payload)?;
            instruction::move_lamports::process_move_lamports(accounts, lamports)
        }

        // --------------------------------------------------------------------
        // Misc
        // --------------------------------------------------------------------
       StakeInstruction::GetMinimumDelegation => {
            msg!("Instruction: GetMinimumDelegation");
            let value = crate::helpers::get_minimum_delegation();
            let data = value.to_le_bytes();

            #[cfg(feature = "solana")]
            {
                pinocchio::program::set_return_data(&data);
            }
            #[cfg(not(feature = "solana"))]
            unsafe {
                pinocchio::syscalls::sol_set_return_data(data.as_ptr(), data.len() as u64);
            }

            Ok(())
        }

        StakeInstruction::DeactivateDelinquent => {
            msg!("Instruction: DeactivateDelinquent");
            instruction::deactivate_delinquent::process_deactivate_delinquent(accounts)
        }

        #[allow(deprecated)]
        StakeInstruction::Redelegate => Err(ProgramError::InvalidInstructionData),
    }
}