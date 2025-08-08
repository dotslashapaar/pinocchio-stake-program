use crate::instruction::{self, StakeInstruction};
use pinocchio::{account_info::AccountInfo, msg, program_error::ProgramError, pubkey::Pubkey, ProgramResult};

#[cfg(not(feature = "no-entrypoint"))]
pinocchio::program_entrypoint!(process_instruction);

    fn process_instruction(
        _program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let (tag, rest) = instruction_data
            .split_first()
            .ok_or(ProgramError::InvalidInstructionData)?;

        match StakeInstruction::try_from(tag)? {
            StakeInstruction::DeactivateDelinquent => {
                instruction::deactivate_delinquent::process_deactivate_delinquent(accounts)
            }

            StakeInstruction::MoveLamports => {
                if rest.len() != 8 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                let lamports = u64::from_le_bytes(rest.try_into().unwrap());
                instruction::move_lamports::process_move_lamports(accounts, lamports)
            }

            StakeInstruction::Initialize => {
                msg!("Instruction: Initialize");
                todo!()
            }
            StakeInstruction::Authorize => {
                msg!("Instruction: Authorize");
                todo!()
            }
            StakeInstruction::DelegateStake => {
                msg!("Instruction: DelegateStake");
                todo!()
            }
            StakeInstruction::Split => {
                msg!("Instruction: Split");
                todo!()
            }
            StakeInstruction::Withdraw => {
                msg!("Instruction: Withdraw");
                todo!()
            }
            StakeInstruction::Deactivate => {
                msg!("Instruction: Deactivate");
                todo!()
            }
            StakeInstruction::SetLockup => {
                msg!("Instruction: SetLockup");
                todo!()
            }
            StakeInstruction::Merge => {
                msg!("Instruction: Merge");
                todo!()
            }
            StakeInstruction::AuthorizeWithSeed => {
                msg!("Instruction: AuthorizeWithSeed");
                todo!()
            }
            StakeInstruction::InitializeChecked => {
                msg!("Instruction: InitializeChecked");
                todo!()
            }
            StakeInstruction::AuthorizeChecked => {
                msg!("Instruction: AuthorizeChecked");
                todo!()
            }
            StakeInstruction::AuthorizeCheckedWithSeed => {
                msg!("Instruction: AuthorizeCheckedWithSeed");
                todo!()
            }
            StakeInstruction::SetLockupChecked => {
                msg!("Instruction: SetLockupChecked");
                todo!()
            }
            StakeInstruction::GetMinimumDelegation => {
                msg!("Instruction: GetMinimumDelegation");
                todo!()
            }
            #[allow(deprecated)]
            StakeInstruction::Redelegate => Err(ProgramError::InvalidInstructionData),
            StakeInstruction::MoveStake => {
                if rest.len() != 8 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                msg!("Instruction: MoveStake");
                todo!()
            }
        }
    }