use crate::{
    helpers::get_minimum_delegation,
    instruction::{self},
    state::{
        accounts::{AuthorizeCheckedWithSeedData, AuthorizeWithSeedData},
        StakeAuthorize,
    },
};
use crate::error::{to_program_error, StakeError};
#[cfg(feature = "std")]
use bincode;
use pinocchio::{
    account_info::AccountInfo, msg, program_entrypoint, program_error::ProgramError,
    pubkey::Pubkey, ProgramResult,
};

// Entrypoint macro
program_entrypoint!(process_instruction);

#[inline(always)]
fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    // Enforce correct program id for consensus parity with native
    let expected_id = Pubkey::try_from(&crate::ID[..]).map_err(|_| ProgramError::IncorrectProgramId)?;
    if *_program_id != expected_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    // Decode StakeInstruction via bincode when building with std (host/dev)
    #[cfg(feature = "std")]
    {
        if let Ok(wire_ix) = bincode::deserialize::<wire::StakeInstruction>(instruction_data) {
            // EpochRewards gating
            if epoch_rewards_active() {
                if !matches!(wire_ix, wire::StakeInstruction::GetMinimumDelegation) {
                    return Err(to_program_error(StakeError::EpochRewardsActive));
                }
            }
            return dispatch_wire_instruction(accounts, wire_ix);
        }
    }

    // Fallback to legacy single-byte discriminator + raw payload
    let (disc, payload) = instruction_data
        .split_first()
        .ok_or(ProgramError::InvalidInstructionData)?;

    // Helper for u64 payloads (lamports, etc.)
    let read_u64 = |data: &[u8]| -> Result<u64, ProgramError> {
        if data.len() != 8 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(data);
        Ok(u64::from_le_bytes(buf))
    };

    match crate::instruction::StakeInstruction::try_from(disc)? {
        // --------------------------------------------------------------------
        // Initialization
        // --------------------------------------------------------------------
        crate::instruction::StakeInstruction::Initialize => {
            msg!("Instruction: Initialize");
            if epoch_rewards_active() {
                return Err(to_program_error(StakeError::EpochRewardsActive));
            }
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
        crate::instruction::StakeInstruction::InitializeChecked => {
            msg!("Instruction: InitializeChecked");
            if epoch_rewards_active() {
                return Err(to_program_error(StakeError::EpochRewardsActive));
            }
            // No payload; authorities are passed as accounts
            instruction::initialize_checked::process_initialize_checked(accounts)
        }

        // --------------------------------------------------------------------
        // Authorization (4 variants)
        // --------------------------------------------------------------------
        crate::instruction::StakeInstruction::Authorize => {
            msg!("Instruction: Authorize");
            if epoch_rewards_active() {
                return Err(to_program_error(StakeError::EpochRewardsActive));
            }
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

        crate::instruction::StakeInstruction::AuthorizeWithSeed => {
            msg!("Instruction: AuthorizeWithSeed");
            if epoch_rewards_active() {
                return Err(to_program_error(StakeError::EpochRewardsActive));
            }
            let args = AuthorizeWithSeedData::parse(payload)?;
            
            instruction::process_authorized_with_seeds::process_authorized_with_seeds(accounts, args)
        }

        crate::instruction::StakeInstruction::AuthorizeChecked => {
            msg!("Instruction: AuthorizeChecked");
            if epoch_rewards_active() {
                return Err(to_program_error(StakeError::EpochRewardsActive));
            }
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

        crate::instruction::StakeInstruction::AuthorizeCheckedWithSeed => {
            msg!("Instruction: AuthorizeCheckedWithSeed");
            if epoch_rewards_active() {
                return Err(to_program_error(StakeError::EpochRewardsActive));
            }
            let args = AuthorizeCheckedWithSeedData::parse(payload)?;
            instruction::process_authorize_checked_with_seed::process_authorize_checked_with_seed(
                accounts,
                args,
            )
        }

        // --------------------------------------------------------------------
        // Stake lifecycle
        // --------------------------------------------------------------------
        crate::instruction::StakeInstruction::DelegateStake => {
            msg!("Instruction: DelegateStake");
            if epoch_rewards_active() {
                return Err(to_program_error(StakeError::EpochRewardsActive));
            }
            // No payload; stake, vote, clock, history, config, auth are provided as accounts
            instruction::process_delegate::process_delegate(accounts)
        }

        crate::instruction::StakeInstruction::Split => {
            msg!("Instruction: Split");
            if epoch_rewards_active() {
                return Err(to_program_error(StakeError::EpochRewardsActive));
            }
            // Split carries the lamports to split
            let lamports = read_u64(payload)?;
            instruction::split::process_split(accounts, lamports)
        }

        crate::instruction::StakeInstruction::Withdraw => {
            msg!("Instruction: Withdraw");
            if epoch_rewards_active() {
                return Err(to_program_error(StakeError::EpochRewardsActive));
            }
            let lamports = read_u64(payload)?;
            instruction::withdraw::process_withdraw(accounts, lamports)
        }

        crate::instruction::StakeInstruction::Deactivate => {
            msg!("Instruction: Deactivate");
            if epoch_rewards_active() {
                return Err(to_program_error(StakeError::EpochRewardsActive));
            }
            instruction::deactivate::process_deactivate(accounts)
        }

        // --------------------------------------------------------------------
        // Lockup (2 variants)
        // --------------------------------------------------------------------
        crate::instruction::StakeInstruction::SetLockup => {
            msg!("Instruction: SetLockup");
            if epoch_rewards_active() {
                return Err(to_program_error(StakeError::EpochRewardsActive));
            }
            // Payload carries lockup args; handler parses internally
            instruction::process_set_lockup::process_set_lockup(accounts, payload)
        }

        crate::instruction::StakeInstruction::SetLockupChecked => {
            msg!("Instruction: SetLockupChecked");
            if epoch_rewards_active() {
                return Err(to_program_error(StakeError::EpochRewardsActive));
            }
            instruction::process_set_lockup_checked::process_set_lockup_checked(accounts, payload)
        }

        // --------------------------------------------------------------------
        // Merge
        // --------------------------------------------------------------------
        crate::instruction::StakeInstruction::Merge => {
            msg!("Instruction: Merge");
            if epoch_rewards_active() {
                return Err(to_program_error(StakeError::EpochRewardsActive));
            }
            // No payload
            instruction::merge_dedicated::process_merge(accounts)
        }

        // --------------------------------------------------------------------
        // Move stake/lamports (post feature-activation)
        // --------------------------------------------------------------------
        crate::instruction::StakeInstruction::MoveStake => {
            msg!("Instruction: MoveStake");
            if epoch_rewards_active() {
                return Err(to_program_error(StakeError::EpochRewardsActive));
            }
            let lamports = read_u64(payload)?;
            instruction::process_move_stake::process_move_stake(accounts, lamports)
        }
        crate::instruction::StakeInstruction::MoveLamports => {
            msg!("Instruction: MoveLamports");
            if epoch_rewards_active() {
                return Err(to_program_error(StakeError::EpochRewardsActive));
            }
            let lamports = read_u64(payload)?;
            instruction::move_lamports::process_move_lamports(accounts, lamports)
        }

        // --------------------------------------------------------------------
        // Misc
        // --------------------------------------------------------------------
       crate::instruction::StakeInstruction::GetMinimumDelegation => {
            msg!("Instruction: GetMinimumDelegation");
            let value = crate::helpers::get_minimum_delegation();
            let data = value.to_le_bytes();

           #[cfg(not(feature = "std"))]
    {
        // Return data for on-chain consumers
        pinocchio::program::set_return_data(&data);
    }

    // Host builds (std): no-op (no return data channel)
    #[cfg(feature = "std")]
    {
        // No-op; tests can read `value` directly if needed
        let _ = data;
    }

            Ok(())
        }

        crate::instruction::StakeInstruction::DeactivateDelinquent => {
            msg!("Instruction: DeactivateDelinquent");
            instruction::deactivate_delinquent::process_deactivate_delinquent(accounts)
        }

        #[allow(deprecated)]
        crate::instruction::StakeInstruction::Redelegate => Err(ProgramError::InvalidInstructionData),
    }
}

// Wire decoding for StakeInstruction (bincode) for std builds
#[cfg(feature = "std")]
mod wire {
    use serde::{Deserialize, Serialize};
    use super::*;

    pub type WirePubkey = [u8; 32];
    impl From<WirePubkey> for Pubkey { fn from(w: WirePubkey) -> Self { Pubkey::new_from_array(w) } }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Authorized { pub staker: WirePubkey, pub withdrawer: WirePubkey }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Lockup { pub unix_timestamp: i64, pub epoch: u64, pub custodian: WirePubkey }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum StakeAuthorize { Staker, Withdrawer }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct LockupArgs { pub unix_timestamp: Option<i64>, pub epoch: Option<u64>, pub custodian: Option<WirePubkey> }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct LockupCheckedArgs { pub unix_timestamp: Option<i64>, pub epoch: Option<u64> }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct AuthorizeWithSeedArgs { pub new_authorized_pubkey: WirePubkey, pub stake_authorize: StakeAuthorize, pub authority_seed: String, pub authority_owner: WirePubkey }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct AuthorizeCheckedWithSeedArgs { pub stake_authorize: StakeAuthorize, pub authority_seed: String, pub authority_owner: WirePubkey }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub enum StakeInstruction {
        Initialize(Authorized, Lockup),
        Authorize(WirePubkey, StakeAuthorize),
        DelegateStake,
        Split(u64),
        Withdraw(u64),
        Deactivate,
        SetLockup(LockupArgs),
        Merge,
        AuthorizeWithSeed(AuthorizeWithSeedArgs),
        InitializeChecked,
        AuthorizeChecked(StakeAuthorize),
        AuthorizeCheckedWithSeed(AuthorizeCheckedWithSeedArgs),
        SetLockupChecked(LockupCheckedArgs),
        GetMinimumDelegation,
        DeactivateDelinquent,
        #[deprecated]
        Redelegate,
        MoveStake(u64),
        MoveLamports(u64),
    }
}

#[cfg(feature = "std")]
fn dispatch_wire_instruction(accounts: &[AccountInfo], ix: wire::StakeInstruction) -> ProgramResult {
    use wire::*;
    match ix {
        StakeInstruction::Initialize(auth, l) => {
            msg!("Instruction: Initialize");
            let authorized = crate::state::accounts::Authorized { staker: Pubkey::from(auth.staker), withdrawer: Pubkey::from(auth.withdrawer) };
            let lockup = crate::state::state::Lockup { unix_timestamp: l.unix_timestamp, epoch: l.epoch, custodian: Pubkey::from(l.custodian) };
            instruction::initialize::initialize(accounts, authorized, lockup)
        }
        StakeInstruction::Authorize(new_auth, which) => {
            msg!("Instruction: Authorize");
            let typ = match which { StakeAuthorize::Staker => StakeAuthorize::Staker, StakeAuthorize::Withdrawer => StakeAuthorize::Withdrawer };
            instruction::authorize::process_authorize(accounts, Pubkey::from(new_auth), typ)
        }
        StakeInstruction::DelegateStake => {
            msg!("Instruction: DelegateStake");
            instruction::process_delegate::process_delegate(accounts)
        }
        StakeInstruction::Split(lamports) => {
            msg!("Instruction: Split");
            instruction::split::process_split(accounts, lamports)
        }
        StakeInstruction::Withdraw(lamports) => {
            msg!("Instruction: Withdraw");
            instruction::withdraw::process_withdraw(accounts, lamports)
        }
        StakeInstruction::Deactivate => {
            msg!("Instruction: Deactivate");
            instruction::deactivate::process_deactivate(accounts)
        }
        StakeInstruction::SetLockup(args) => {
            msg!("Instruction: SetLockup");
            // Translate into our SetLockupData shape
            let data = crate::state::accounts::SetLockupData {
                unix_timestamp: args.unix_timestamp,
                epoch: args.epoch,
                custodian: args.custodian.map(|c| Pubkey::from(c)),
            };
            instruction::process_set_lockup::process_set_lockup_parsed(accounts, data)
        }
        StakeInstruction::Merge => {
            msg!("Instruction: Merge");
            instruction::merge_dedicated::process_merge(accounts)
        }
        StakeInstruction::AuthorizeWithSeed(args) => {
            msg!("Instruction: AuthorizeWithSeed");
            let new_authorized = Pubkey::from(args.new_authorized_pubkey);
            let stake_authorize = match args.stake_authorize { StakeAuthorize::Staker => StakeAuthorize::Staker, StakeAuthorize::Withdrawer => StakeAuthorize::Withdrawer };
            let authority_owner = Pubkey::from(args.authority_owner);
            let seed_vec = args.authority_seed.into_bytes();
            let data = AuthorizeWithSeedData { new_authorized, stake_authorize, authority_seed: &seed_vec, authority_owner };
            // Keep seed_vec alive across the call
            let res = instruction::process_authorized_with_seeds::process_authorized_with_seeds(accounts, data);
            core::mem::drop(seed_vec);
            res
        }
        StakeInstruction::InitializeChecked => {
            msg!("Instruction: InitializeChecked");
            instruction::initialize_checked::process_initialize_checked(accounts)
        }
        StakeInstruction::AuthorizeChecked(which) => {
            msg!("Instruction: AuthorizeChecked");
            let typ = match which { StakeAuthorize::Staker => StakeAuthorize::Staker, StakeAuthorize::Withdrawer => StakeAuthorize::Withdrawer };
            instruction::authorize_checked::process_authorize_checked(accounts, typ)
        }
        StakeInstruction::AuthorizeCheckedWithSeed(args) => {
            msg!("Instruction: AuthorizeCheckedWithSeed");
            let stake_authorize = match args.stake_authorize { StakeAuthorize::Staker => StakeAuthorize::Staker, StakeAuthorize::Withdrawer => StakeAuthorize::Withdrawer };
            let authority_owner = Pubkey::from(args.authority_owner);
            let seed_vec = args.authority_seed.into_bytes();
            let data = AuthorizeCheckedWithSeedData { stake_authorize, authority_seed: &seed_vec, authority_owner };
            let res = instruction::process_authorize_checked_with_seed::process_authorize_checked_with_seed(accounts, data);
            core::mem::drop(seed_vec);
            res
        }
        StakeInstruction::SetLockupChecked(args) => {
            msg!("Instruction: SetLockupChecked");
            // Handler parses optional new custodian from accounts
            let _ = args; // values applied inside handler based on accounts and lockup status
            instruction::process_set_lockup_checked::process_set_lockup_checked(accounts, &[])
        }
        StakeInstruction::GetMinimumDelegation => {
            msg!("Instruction: GetMinimumDelegation");
            let value = crate::helpers::get_minimum_delegation();
            let data = value.to_le_bytes();
            #[cfg(not(feature = "std"))]
            { pinocchio::program::set_return_data(&data); }
            Ok(())
        }
        StakeInstruction::DeactivateDelinquent => {
            msg!("Instruction: DeactivateDelinquent");
            instruction::deactivate_delinquent::process_deactivate_delinquent(accounts)
        }
        #[allow(deprecated)]
        StakeInstruction::Redelegate => Err(ProgramError::InvalidInstructionData),
        StakeInstruction::MoveStake(lamports) => {
            msg!("Instruction: MoveStake");
            instruction::process_move_stake::process_move_stake(accounts, lamports)
        }
        StakeInstruction::MoveLamports(lamports) => {
            msg!("Instruction: MoveLamports");
            instruction::move_lamports::process_move_lamports(accounts, lamports)
        }
    }
}

// ---- EpochRewards gating (attempt best-effort sysvar read) ----
fn epoch_rewards_active() -> bool { false }
