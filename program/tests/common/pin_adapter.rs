use solana_program_test::BanksClient;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    stake::{
        instruction as sdk_ixn,
        program::id as stake_program_id,
        state::{Authorized, Lockup, Meta, Stake, StakeAuthorize},
    },
};

pub mod ixn {
    use super::*;

    #[inline]
    fn rebuild_accounts_order(accounts: &mut Vec<AccountMeta>, head: &[Pubkey]) {
        let mut new = Vec::with_capacity(accounts.len());
        for k in head {
            if let Some(pos) = accounts.iter().position(|am| &am.pubkey == k) {
                new.push(accounts.remove(pos));
            }
        }
        new.append(accounts);
        *accounts = new;
    }

    #[inline]
    fn role_byte(role: &StakeAuthorize) -> u8 {
        match role {
            StakeAuthorize::Staker => 0,
            StakeAuthorize::Withdrawer => 1,
        }
    }

    pub fn get_minimum_delegation() -> Instruction {
        let mut ix = sdk_ixn::get_minimum_delegation();
        ix.data = vec![13];
        ix
    }

    pub fn initialize(stake: &Pubkey, authorized: &Authorized, lockup: &Lockup) -> Instruction {
        let mut ix = sdk_ixn::initialize(stake, authorized, lockup);
        let mut data = Vec::with_capacity(1 + 112);
        data.push(0);
        data.extend_from_slice(&authorized.staker.to_bytes());
        data.extend_from_slice(&authorized.withdrawer.to_bytes());
        data.extend_from_slice(&lockup.unix_timestamp.to_le_bytes());
        data.extend_from_slice(&lockup.epoch.to_le_bytes());
        data.extend_from_slice(&lockup.custodian.to_bytes());
        ix.data = data;
        ix
    }

    pub fn initialize_checked(stake: &Pubkey, authorized: &Authorized) -> Instruction {
        let mut ix = sdk_ixn::initialize_checked(stake, authorized);
        ix.data = vec![9];
        ix
    }

    pub fn authorize(
        stake: &Pubkey,
        authority: &Pubkey,
        new_authorized: &Pubkey,
        role: StakeAuthorize,
        custodian: Option<&Pubkey>,
    ) -> Instruction {
        let mut ix = sdk_ixn::authorize(stake, authority, new_authorized, role, custodian);
        let mut accts = ix.accounts.clone();
        rebuild_accounts_order(&mut accts, &[*stake, solana_sdk::sysvar::clock::id()]);
        ix.accounts = accts;
        let mut data = Vec::with_capacity(1 + 33);
        data.push(1);
        data.extend_from_slice(&new_authorized.to_bytes());
        data.push(role_byte(&role));
        ix.data = data;
        ix
    }

    pub fn authorize_checked(
        stake: &Pubkey,
        authority: &Pubkey,
        new_authorized: &Pubkey,
        role: StakeAuthorize,
        custodian: Option<&Pubkey>,
    ) -> Instruction {
        let mut ix = sdk_ixn::authorize_checked(stake, authority, new_authorized, role, custodian);
        let mut accts = ix.accounts.clone();
        rebuild_accounts_order(
            &mut accts,
            &[*stake, solana_sdk::sysvar::clock::id(), *authority, *new_authorized],
        );
        ix.accounts = accts;
        ix.data = vec![10, role_byte(&role)];
        ix
    }

    pub fn authorize_checked_with_seed(
        stake: &Pubkey,
        base: &Pubkey,
        seed: String,
        owner: &Pubkey,
        new_authorized: &Pubkey,
        role: StakeAuthorize,
        custodian: Option<&Pubkey>,
    ) -> Instruction {
        let mut ix = sdk_ixn::authorize_checked_with_seed(
            stake,
            base,
            seed.clone(),
            owner,
            new_authorized,
            role,
            custodian,
        );
        let mut accts = ix.accounts.clone();
        rebuild_accounts_order(
            &mut accts,
            &[*stake, *base, solana_sdk::sysvar::clock::id(), *new_authorized],
        );
        ix.accounts = accts;
        let seed_bytes = seed.as_bytes();
        let mut data = Vec::with_capacity(1 + 32 + 1 + 1 + seed_bytes.len() + 32);
        data.push(11);
        data.extend_from_slice(&new_authorized.to_bytes());
        data.push(role_byte(&role));
        data.push(u8::try_from(seed_bytes.len()).unwrap());
        data.extend_from_slice(seed_bytes);
        data.extend_from_slice(&owner.to_bytes());
        ix.data = data;
        ix
    }

    // Non-checked with-seed variant: base signs; new_authorized does not need to sign
    pub fn authorize_with_seed(
        stake: &Pubkey,
        base: &Pubkey,
        seed: String,
        owner: &Pubkey,
        new_authorized: &Pubkey,
        role: StakeAuthorize,
        _custodian: Option<&Pubkey>,
    ) -> Instruction {
        // Build explicit minimal metas for non-checked variant: [stake (w), base (s), clock]
        let mut ix = Instruction {
            program_id: stake_program_id(),
            accounts: vec![
                AccountMeta::new(*stake, false),
                AccountMeta::new_readonly(*base, true),
                AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
            ],
            data: vec![],
        };
        let seed_bytes = seed.as_bytes();
        let mut data = Vec::with_capacity(1 + 32 + 1 + 1 + seed_bytes.len() + 32);
        data.push(8); // non-checked discriminant
        data.extend_from_slice(&new_authorized.to_bytes());
        data.push(role_byte(&role));
        data.push(u8::try_from(seed_bytes.len()).unwrap());
        data.extend_from_slice(seed_bytes);
        data.extend_from_slice(&owner.to_bytes());
        ix.data = data;
        ix
    }

    pub fn set_lockup_checked(stake: &Pubkey, args: &solana_sdk::stake::instruction::LockupArgs, signer: &Pubkey) -> Instruction {
        let mut ix = sdk_ixn::set_lockup_checked(stake, args, signer);
        let mut data = Vec::with_capacity(1 + 1 + 16);
        data.push(12);
        let mut flags = 0u8;
        if args.unix_timestamp.is_some() { flags |= 0x01; }
        if args.epoch.is_some() { flags |= 0x02; }
        data.push(flags);
        if let Some(ts) = args.unix_timestamp { data.extend_from_slice(&ts.to_le_bytes()); }
        if let Some(ep) = args.epoch { data.extend_from_slice(&ep.to_le_bytes()); }
        ix.data = data;
        ix
    }

    pub fn delegate_stake(stake: &Pubkey, staker: &Pubkey, vote: &Pubkey) -> Instruction {
        let mut ix = sdk_ixn::delegate_stake(stake, staker, vote);
        // Expected by program: [stake, vote, clock, stake_history, stake_config, ...]
        let mut accts = ix.accounts.clone();
        rebuild_accounts_order(
            &mut accts,
            &[
                *stake,
                *vote,
                solana_sdk::sysvar::clock::id(),
                solana_sdk::sysvar::stake_history::id(),
                solana_sdk::stake::config::id(),
            ],
        );
        // Ensure stake_config is present (some SDKs may omit it from delegate metas)
        if !accts.iter().any(|am| am.pubkey == solana_sdk::stake::config::id()) {
            accts.push(AccountMeta::new_readonly(solana_sdk::stake::config::id(), false));
        }
        ix.accounts = accts;
        ix.data = vec![2];
        ix
    }

    pub fn split(stake: &Pubkey, authority: &Pubkey, lamports: u64, split_dest: &Pubkey) -> Vec<Instruction> {
        // Build via SDK and translate the stake-program instruction payload and
        // account ordering to our program's format. Also, ensure the stake
        // instruction is first in the vector so tests can `.next()` it.
        let mut v = sdk_ixn::split(stake, authority, lamports, split_dest);

        // Patch stake-program instruction(s)
        for i in &mut v {
            if i.program_id == stake_program_id() {
                // Ensure account ordering starts with [stake, split_dest, authority]
                let mut accts = i.accounts.clone();
                rebuild_accounts_order(&mut accts, &[*stake, *split_dest, *authority]);
                i.accounts = accts;
                // Overwrite data with Pinocchio discriminator + lamports
                let mut data = Vec::with_capacity(1 + 8);
                data.push(3);
                data.extend_from_slice(&lamports.to_le_bytes());
                i.data = data;
            }
        }

        v
    }

    pub fn withdraw(
        stake: &Pubkey,
        withdrawer: &Pubkey,
        recipient: &Pubkey,
        lamports: u64,
        custodian: Option<&Pubkey>,
    ) -> Instruction {
        let mut ix = sdk_ixn::withdraw(stake, withdrawer, recipient, lamports, custodian);
        // Expected by program: [stake, recipient, clock, stake_history, withdrawer, (custodian?)]
        let mut accts = ix.accounts.clone();
        let mut head = vec![
            *stake,
            *recipient,
            solana_sdk::sysvar::clock::id(),
            solana_sdk::sysvar::stake_history::id(),
            *withdrawer,
        ];
        if let Some(c) = custodian { head.push(*c); }
        rebuild_accounts_order(&mut accts, &head);
        ix.accounts = accts;
        let mut data = Vec::with_capacity(1 + 8);
        data.push(4);
        data.extend_from_slice(&lamports.to_le_bytes());
        ix.data = data;
        ix
    }

    pub fn deactivate_stake(stake: &Pubkey, staker: &Pubkey) -> Instruction {
        let mut ix = sdk_ixn::deactivate_stake(stake, staker);
        // Expected by program: [stake, clock, ...]
        let mut accts = ix.accounts.clone();
        rebuild_accounts_order(&mut accts, &[*stake, solana_sdk::sysvar::clock::id()]);
        ix.accounts = accts;
        ix.data = vec![5];
        ix
    }

    // Convenience alias matching native name
    pub fn deactivate(stake: &Pubkey, staker: &Pubkey) -> Instruction {
        deactivate_stake(stake, staker)
    }

    pub fn merge(dest: &Pubkey, src: &Pubkey, authority: &Pubkey) -> Vec<Instruction> {
        let mut v = sdk_ixn::merge(dest, src, authority);
        for i in &mut v {
            if i.program_id == stake_program_id() {
                let mut accts = i.accounts.clone();
                rebuild_accounts_order(
                    &mut accts,
                    &[*dest, *src, solana_sdk::sysvar::clock::id(), solana_sdk::sysvar::stake_history::id()],
                );
                i.accounts = accts;
                i.data = vec![7];
            }
        }
        v
    }

    pub fn move_stake(source: &Pubkey, dest: &Pubkey, staker: &Pubkey, lamports: u64) -> Instruction {
        let mut ix = sdk_ixn::move_stake(source, dest, staker, lamports);
        // Replace metas with exactly what our program expects
        ix.accounts = vec![
            AccountMeta::new(*source, false),
            AccountMeta::new(*dest, false),
            AccountMeta::new_readonly(*staker, true),
        ];
        let mut data = Vec::with_capacity(1 + 8);
        data.push(16);
        data.extend_from_slice(&lamports.to_le_bytes());
        ix.data = data;
        ix
    }

    pub fn move_lamports(source: &Pubkey, dest: &Pubkey, staker: &Pubkey, lamports: u64) -> Instruction {
        let mut ix = sdk_ixn::move_lamports(source, dest, staker, lamports);
        // Expected by program: [source, dest, staker]
        let mut accts = ix.accounts.clone();
        rebuild_accounts_order(&mut accts, &[*source, *dest, *staker]);
        ix.accounts = accts;
        let mut data = Vec::with_capacity(1 + 8);
        data.push(17);
        data.extend_from_slice(&lamports.to_le_bytes());
        ix.data = data;
        ix
    }

    // DeactivateDelinquent: [stake, delinquent_vote, reference_vote]
    pub fn deactivate_delinquent(stake: &Pubkey, delinquent_vote: &Pubkey, reference_vote: &Pubkey) -> Instruction {
        let mut ix = Instruction {
            program_id: stake_program_id(),
            accounts: vec![
                AccountMeta::new(*stake, false),
                AccountMeta::new_readonly(*delinquent_vote, false),
                AccountMeta::new_readonly(*reference_vote, false),
            ],
            data: vec![14u8],
        };
        // Ensure order exactly as program expects
        let mut accts = ix.accounts.clone();
        rebuild_accounts_order(&mut accts, &[*stake, *delinquent_vote, *reference_vote]);
        ix.accounts = accts;
        ix
    }
}

// Re-export ixn::* so tests can `use crate::common::pin_adapter as ixn;`
pub use ixn::*;

// ---------- State helpers ----------
pub async fn get_stake_account(
    banks_client: &mut BanksClient,
    pubkey: &Pubkey,
) -> (Meta, Option<Stake>, u64) {
    use pinocchio_stake::state as pstate;
    let stake_account = banks_client.get_account(*pubkey).await.unwrap().unwrap();
    let lamports = stake_account.lamports;
    let st = pstate::stake_state_v2::StakeStateV2::deserialize(&stake_account.data).unwrap();
    match st {
        pstate::stake_state_v2::StakeStateV2::Initialized(meta) => {
            let meta_sdk = Meta {
                authorized: Authorized {
                    staker: Pubkey::new_from_array(meta.authorized.staker),
                    withdrawer: Pubkey::new_from_array(meta.authorized.withdrawer),
                },
                rent_exempt_reserve: u64::from_le_bytes(meta.rent_exempt_reserve),
                lockup: Lockup {
                    unix_timestamp: meta.lockup.unix_timestamp,
                    epoch: meta.lockup.epoch,
                    custodian: Pubkey::new_from_array(meta.lockup.custodian),
                },
            };
            (meta_sdk, None, lamports)
        }
        pstate::stake_state_v2::StakeStateV2::Stake(meta, stake, _flags) => {
            let meta_sdk = Meta {
                authorized: Authorized {
                    staker: Pubkey::new_from_array(meta.authorized.staker),
                    withdrawer: Pubkey::new_from_array(meta.authorized.withdrawer),
                },
                rent_exempt_reserve: u64::from_le_bytes(meta.rent_exempt_reserve),
                lockup: Lockup {
                    unix_timestamp: meta.lockup.unix_timestamp,
                    epoch: meta.lockup.epoch,
                    custodian: Pubkey::new_from_array(meta.lockup.custodian),
                },
            };
            let del = &stake.delegation;
            let delegation_sdk = solana_sdk::stake::state::Delegation {
                voter_pubkey: Pubkey::new_from_array(del.voter_pubkey),
                stake: u64::from_le_bytes(del.stake),
                activation_epoch: u64::from_le_bytes(del.activation_epoch),
                deactivation_epoch: u64::from_le_bytes(del.deactivation_epoch),
                warmup_cooldown_rate: f64::from_bits(u64::from_le_bytes(del.warmup_cooldown_rate)),
            };
            let stake_sdk = Stake {
                delegation: delegation_sdk,
                credits_observed: u64::from_le_bytes(stake.credits_observed),
            };
            (meta_sdk, Some(stake_sdk), lamports)
        }
        pstate::stake_state_v2::StakeStateV2::Uninitialized => panic!("panic: uninitialized"),
        _ => unimplemented!(),
    }
}

pub async fn get_stake_account_rent(banks_client: &mut BanksClient) -> u64 {
    let rent = banks_client.get_rent().await.unwrap();
    rent.minimum_balance(pinocchio_stake::state::stake_state_v2::StakeStateV2::size_of())
}

pub fn encode_program_stake_state(st: &pinocchio_stake::state::stake_state_v2::StakeStateV2) -> Vec<u8> {
    let mut buf = vec![0u8; pinocchio_stake::state::stake_state_v2::StakeStateV2::size_of()];
    pinocchio_stake::state::stake_state_v2::StakeStateV2::serialize(st, &mut buf)
        .expect("serialize stake state");
    buf
}

// ---------- Error helpers ----------
pub mod err {
    use solana_sdk::{program_error::ProgramError, stake::instruction::StakeError};

    pub fn matches_stake_error(e: &ProgramError, expected: StakeError) -> bool {
        match (e, expected.clone()) {
            (ProgramError::Custom(0x11), StakeError::AlreadyDeactivated) => true,
            (ProgramError::Custom(0x12), StakeError::InsufficientDelegation) => true,
            (ProgramError::Custom(0x13), StakeError::VoteAddressMismatch) => true,
            (ProgramError::Custom(0x14), StakeError::MergeMismatch) => true,
            (ProgramError::Custom(0x15), StakeError::LockupInForce) => true,
            (ProgramError::Custom(0x18), StakeError::TooSoonToRedelegate) => true,
            _ => *e == expected.into(),
        }
    }
}
