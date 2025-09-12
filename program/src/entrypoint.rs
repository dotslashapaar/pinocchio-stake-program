use crate::{instruction::{self, StakeInstruction}, state::{AuthorizeCheckedWithSeedData, AuthorizeWithSeedData, StakeAuthorize}};
use pinocchio::{
    account_info::AccountInfo, default_panic_handler, msg, no_allocator, program_entrypoint,
    program_error::ProgramError, pubkey::Pubkey, ProgramResult,
};


// This is the entrypoint for the program.
program_entrypoint!(process_instruction);
//Do not allocate memory.
no_allocator!();
// Use the no_std panic handler.
default_panic_handler!();

#[inline(always)]
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let (ix_disc, instruction_data) = instruction_data
        .split_first()
        .ok_or(ProgramError::InvalidInstructionData)?;

    match StakeInstruction::try_from(ix_disc)? {
        StakeInstruction::Initialize => {
            pinocchio::msg!("Instruction: Initialize");
            todo!()
        }
        StakeInstruction::Authorize => {
        msg!("Instruction: Authorize");

        // Expect exactly 33 bytes: [0..32]=new pubkey, [32]=role
        if instruction_data.len() != 33 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let mut pk = [0u8; 32];
        pk.copy_from_slice(&instruction_data[..32]);
       let new_authority = Pubkey::try_from(&instruction_data[..32])
    .map_err(|_| ProgramError::InvalidInstructionData)?;

        let authority_type = match instruction_data[32] {
            0 => StakeAuthorize::Staker,
            1 => StakeAuthorize::Withdrawer,
            _ => return Err(ProgramError::InvalidInstructionData),
        };

        // Typed handler (native-style)
        // fn process_authorize(accounts: &[AccountInfo], new_authority: Pubkey, authority_type: StakeAuthorize) -> ProgramResult
        instruction::authorize::process_authorize(accounts, new_authority, authority_type)
    }

    StakeInstruction::AuthorizeWithSeed => {
        msg!("Instruction: AuthorizeWithSeed");

        // Parse into typed struct
        let args = AuthorizeWithSeedData::parse(instruction_data)?;

        // Typed handler (native-style)
        // fn process_authorized_with_seeds(accounts: &[AccountInfo], args: AuthorizeWithSeedData) -> ProgramResult
        instruction::process_authorized_with_seeds::process_authorized_with_seeds(accounts, args)
    }

    StakeInstruction::AuthorizeChecked => {
        msg!("Instruction: AuthorizeChecked");

        // Expect exactly 1 byte: 0=Staker, 1=Withdrawer
        if instruction_data.len() != 1 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let authority_type = match instruction_data[0] {
            0 => StakeAuthorize::Staker,
            1 => StakeAuthorize::Withdrawer,
            _ => return Err(ProgramError::InvalidInstructionData),
        };

        // Typed handler (native-style)
        // fn process_authorize_checked(accounts: &[AccountInfo], authority_type: StakeAuthorize) -> ProgramResult
        instruction::authorize_checked::process_authorize_checked(accounts, authority_type)
    }

    StakeInstruction::AuthorizeCheckedWithSeed => {
        msg!("Instruction: AuthorizeCheckedWithSeed");

        // Parse into typed struct
        let args = AuthorizeCheckedWithSeedData::parse(instruction_data)?;
        let lamports = u64::from_le_bytes(instruction_data.try_into().unwrap());
        // Typed handler (native-style)
        // fn process_authorize_checked_with_seed(accounts: &[AccountInfo], args: AuthorizeCheckedWithSeedData) -> ProgramResult
        instruction::process_authorize_checked_with_seed::process_authorize_checked_with_seed(accounts, args)
    }
        StakeInstruction::DelegateStake => {
            pinocchio::msg!("Instruction: DelegateStake");
            todo!()
        }
        StakeInstruction::Split => {
            pinocchio::msg!("Instruction: Split");
            todo!()
            //instruction::split::process_split()
        }
        StakeInstruction::Withdraw => {
            pinocchio::msg!("Instruction: Withdraw");
           let lamports = u64::from_le_bytes(instruction_data.try_into().unwrap());
            instruction::withdraw::process_withdraw(accounts,lamports)
        }
        StakeInstruction::Deactivate => {
            pinocchio::msg!("Instruction: Deactivate");
            instruction::deactivate::process_deactivate(accounts)
        }
        StakeInstruction::SetLockup => {
            pinocchio::msg!("Instruction: SetLockup");
            instruction::process_set_lockup::process_set_lockup(accounts, instruction_data)
        }
        StakeInstruction::Merge => {
            pinocchio::msg!("Instruction: Merge");
            todo!()
        }
       StakeInstruction::AuthorizeWithSeed => {
            pinocchio::msg!("Instruction: AuthorizeWithSeed");
            todo!()
        }
       StakeInstruction::InitializeChecked => {
            pinocchio::msg!("Instruction: InitializeChecked");
            todo!()
        }
         StakeInstruction::AuthorizeChecked => {
            pinocchio::msg!("Instruction: AuthorizeChecked");
            todo!()
        }
          StakeInstruction::AuthorizeCheckedWithSeed => {
            pinocchio::msg!("Instruction: AuthorizeCheckedWithSeed");
            todo!()
        }
        StakeInstruction::SetLockupChecked => {
            pinocchio::msg!("Instruction: SetLockupChecked");
            todo!()
        }
        StakeInstruction::GetMinimumDelegation => {
            pinocchio::msg!("Instruction: GetMinimumDelegation");
            todo!()
        }
        StakeInstruction::DeactivateDelinquent => {
            pinocchio::msg!("Instruction: DeactivateDelinquent");
            instruction::deactivate_delinquent::process_deactivate_delinquent(accounts)
        }
        #[allow(deprecated)]
        StakeInstruction::Redelegate => Err(ProgramError::InvalidInstructionData),
        // NOTE we assume the program is going live after `move_stake_and_move_lamports_ixs` is
        // activated
        StakeInstruction::MoveStake => {
            pinocchio::msg!("Instruction: MoveStake");
            let lamports = u64::from_le_bytes(instruction_data.try_into().unwrap());
            instruction::move_stake(accounts, lamports)
        }
        StakeInstruction::MoveLamports => {
            pinocchio::msg!("Instruction: MoveLamports");
            if instruction_data.len() != 8 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                let lamports = u64::from_le_bytes(instruction_data.try_into().unwrap());
                instruction::move_lamports::process_move_lamports(accounts, lamports)
        }
    }
}
