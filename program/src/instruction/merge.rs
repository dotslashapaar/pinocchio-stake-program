// === MERGE INSTRUCTION ENTRY POINT ===
//
// This file serves as the main entry point for the merge instruction.
// It follows a clean delegation pattern to keep the main instruction
// simple while containing all complex logic in a dedicated module.
//
// Flow:
// 1. Entrypoint calls instruction::process_merge(accounts)
// 2. This file delegates to merge_dedicated::process_merge(accounts)
// 3. merge_dedicated.rs contains all the actual merge implementation
//
// This pattern allows us to:
// - Keep merge.rs simple and focused
// - Avoid modifying existing files (delegation.rs, stake_history.rs, etc.)
// - Contain all merge-specific logic in one dedicated place
// - Make it easy to swap implementations if needed

use super::merge_dedicated;
use pinocchio::{account_info::AccountInfo, ProgramResult};

/// Main entry point for the merge instruction
///
/// This function is called by the program entrypoint when a merge instruction
/// is received. It immediately delegates to the dedicated merge implementation
/// to keep this interface clean and simple.
///
/// ## Arguments
/// * `accounts` - Array of account infos passed to the instruction:
///   - [0] Destination stake account (writable)
///   - [1] Source stake account (writable)
///   - [2] Clock sysvar account
///   - [3] Stake history sysvar account
///   - [4+] Optional additional signers
///
/// ## Returns
/// * `ProgramResult` - Ok(()) on success, Err(ProgramError) on failure
///
/// ## Flow
/// 1. Receives accounts from instruction processor
/// 2. Delegates immediately to merge_dedicated::process_merge()
/// 3. Returns result back to caller
pub fn process_merge(accounts: &[AccountInfo]) -> ProgramResult {
    // Delegate to the dedicated merge implementation that doesn't modify existing files
    // This keeps the interface clean while containing all complexity in merge_dedicated.rs
    merge_dedicated::process_merge(accounts)
}
