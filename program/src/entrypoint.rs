use crate::instruction::StakeInstruction;
use pinocchio::{
    account_info::AccountInfo, default_panic_handler, no_allocator, program_entrypoint,
    program_error::ProgramError, pubkey::Pubkey, ProgramResult,
};

// This is the entrypoint for the program.
program_entrypoint!(process_instruction);
//Do not allocate memory.
no_allocator!();
// Use the no_std panic handler.
default_panic_handler!();

#[inline(always)]
pub fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let (ix_disc, _instruction_data) = instruction_data
        .split_first()
        .ok_or(ProgramError::InvalidInstructionData)?;

    match StakeInstruction::try_from(ix_disc)? {
        StakeInstruction::Initialize => {
            pinocchio::msg!("Instruction: Initialize");
            todo!()
        }
        StakeInstruction::Authorize => {
            pinocchio::msg!("Instruction: Authorize");
            todo!()
        }
        StakeInstruction::DelegateStake => {
            pinocchio::msg!("Instruction: DelegateStake");
            todo!()
        }
        StakeInstruction::Split => {
            pinocchio::msg!("Instruction: Split");
            todo!()
        }
        StakeInstruction::Withdraw => {
            pinocchio::msg!("Instruction: Withdraw");
            todo!()
        }
        StakeInstruction::Deactivate => {
            pinocchio::msg!("Instruction: Deactivate");
            todo!()
        }
        StakeInstruction::SetLockup => {
            pinocchio::msg!("Instruction: SetLockup");
            todo!()
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
            todo!()
        }
        #[allow(deprecated)]
        StakeInstruction::Redelegate => Err(ProgramError::InvalidInstructionData),
        // NOTE we assume the program is going live after `move_stake_and_move_lamports_ixs` is
        // activated
        StakeInstruction::MoveStake => {
            pinocchio::msg!("Instruction: MoveStake");
            todo!()
        }
        StakeInstruction::MoveLamports => {
            pinocchio::msg!("Instruction: MoveLamports");
            todo!()
        }
    }
}
