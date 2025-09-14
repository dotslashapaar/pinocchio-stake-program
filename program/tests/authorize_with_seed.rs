mod common;
use common::*;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    system_instruction,
};

// AuthorizeCheckedWithSeed: staker authority is a derived PDA (base+seed+owner). Base signs; new staker signs.
#[tokio::test]
async fn authorize_checked_with_seed_staker_success() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    // Accounts
    let stake_acc = Keypair::new();
    let withdrawer = Keypair::new();
    let base = Keypair::new();
    let seed = "seed-for-staker";
    let owner = solana_sdk::system_program::id();
    let derived_staker = Pubkey::create_with_seed(&base.pubkey(), seed, &owner).unwrap();

    // Create stake account owned by our program
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let reserve = rent.minimum_balance(space as usize);

    let create = system_instruction::create_account(
        &ctx.payer.pubkey(),
        &stake_acc.pubkey(),
        reserve,
        space,
        &program_id,
    );
    let msg = Message::new(&[create], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &stake_acc], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // InitializeChecked with base as current staker and real withdrawer (withdrawer must sign)
    let init_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(stake_acc.pubkey(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::rent::id(), false),
            AccountMeta::new_readonly(base.pubkey(), false),
            AccountMeta::new_readonly(withdrawer.pubkey(), true),
        ],
        data: vec![9u8],
    };
    let msg = Message::new(&[init_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Build AuthorizeCheckedWithSeed payload
    // Format: [new_authorized(32)][stake_authorize(1)][seed_len(1)][seed][owner(32)]
    let new_staker = Keypair::new();
    let mut payload = Vec::with_capacity(32 + 1 + 1 + seed.len() + 32);
    payload.extend_from_slice(&new_staker.pubkey().to_bytes());
    payload.push(0u8); // StakeAuthorize::Staker
    payload.push(seed.len() as u8);
    payload.extend_from_slice(seed.as_bytes());
    payload.extend_from_slice(&owner.to_bytes());

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(stake_acc.pubkey(), false),     // stake
            AccountMeta::new_readonly(base.pubkey(), true),  // old authority base (signs)
            AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
            AccountMeta::new_readonly(new_staker.pubkey(), true), // new authority must sign
        ],
        data: {
            let mut d = vec![11u8]; // AuthorizeCheckedWithSeed discriminant
            d.extend_from_slice(&payload);
            d
        },
    };

    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &base, &new_staker], ctx.last_blockhash).unwrap();
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "AuthorizeCheckedWithSeed should succeed: {:?}", res);

    // Verify staker changed
    let acct = ctx
        .banks_client
        .get_account(stake_acc.pubkey())
        .await
        .unwrap()
        .expect("stake account must exist");
    let state = pinocchio_stake::state::stake_state_v2::StakeStateV2::deserialize(&acct.data).unwrap();
    match state {
        pinocchio_stake::state::stake_state_v2::StakeStateV2::Initialized(meta)
        | pinocchio_stake::state::stake_state_v2::StakeStateV2::Stake(meta, _, _) => {
            assert_eq!(meta.authorized.staker, new_staker.pubkey().to_bytes());
            assert_eq!(meta.authorized.withdrawer, withdrawer.pubkey().to_bytes());
        }
        other => panic!("unexpected state after authorize_checked_with_seed: {:?}", other),
    }
}

// Non-checked variant: base signs; new authority does NOT need to sign.
#[tokio::test]
async fn authorize_with_seed_staker_success() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    // Stake account and authorities
    let stake_acc = Keypair::new();
    let withdrawer = Keypair::new();
    let base = Keypair::new();
    let seed = "seed-for-staker";
    let owner = solana_sdk::system_program::id();
    let derived_staker = Pubkey::create_with_seed(&base.pubkey(), seed, &owner).unwrap();

    // Create stake
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let reserve = rent.minimum_balance(space as usize);
    let create = system_instruction::create_account(
        &ctx.payer.pubkey(),
        &stake_acc.pubkey(),
        reserve,
        space,
        &program_id,
    );
    let msg = Message::new(&[create], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &stake_acc], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // InitializeChecked with base as current staker
    let init_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(stake_acc.pubkey(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::rent::id(), false),
            AccountMeta::new_readonly(base.pubkey(), false),
            AccountMeta::new_readonly(withdrawer.pubkey(), true),
        ],
        data: vec![9u8],
    };
    let msg = Message::new(&[init_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Build payload for AuthorizeWithSeed (new staker from payload)
    let new_staker = Keypair::new();
    let mut payload = Vec::with_capacity(32 + 1 + 1 + seed.len() + 32);
    payload.extend_from_slice(&new_staker.pubkey().to_bytes());
    payload.push(0u8); // StakeAuthorize::Staker
    payload.push(seed.len() as u8);
    payload.extend_from_slice(seed.as_bytes());
    payload.extend_from_slice(&owner.to_bytes());

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(stake_acc.pubkey(), false),     // stake
            AccountMeta::new_readonly(base.pubkey(), true),  // base signs
            AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
            // optional custodian not provided
        ],
        data: {
            let mut d = vec![8u8]; // AuthorizeWithSeed discriminant
            d.extend_from_slice(&payload);
            d
        },
    };
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &base], ctx.last_blockhash).unwrap();
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "AuthorizeWithSeed should succeed: {:?}", res);

    // Verify staker changed
    let acct = ctx
        .banks_client
        .get_account(stake_acc.pubkey())
        .await
        .unwrap()
        .expect("stake account must exist");
    let state = pinocchio_stake::state::stake_state_v2::StakeStateV2::deserialize(&acct.data).unwrap();
    match state {
        pinocchio_stake::state::stake_state_v2::StakeStateV2::Initialized(meta)
        | pinocchio_stake::state::stake_state_v2::StakeStateV2::Stake(meta, _, _) => {
            assert_eq!(meta.authorized.staker, new_staker.pubkey().to_bytes());
            assert_eq!(meta.authorized.withdrawer, withdrawer.pubkey().to_bytes());
        }
        other => panic!("unexpected state after authorize_with_seed: {:?}", other),
    }
}
#![cfg(feature = "seed")]
