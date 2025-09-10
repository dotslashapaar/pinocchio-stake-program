// Main entry point for the merge instruction

use super::merge_dedicated;
use pinocchio::{account_info::AccountInfo, ProgramResult};

/// Process merge instruction - delegates to dedicated implementation
pub fn process_merge(accounts: &[AccountInfo]) -> ProgramResult {
    merge_dedicated::process_merge(accounts)
}
