#![cfg(feature = "seed")]
mod common;
use common::*;
use common::pin_adapter as ixn;
use solana_sdk::{
    message::Message,
    pubkey::Pubkey,
    system_instruction,
    stake::state::{Authorized, StakeAuthorize},
};
use solana_sdk::instruction::{Instruction, AccountMeta};

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

    // InitializeChecked with base as current staker and real withdrawer (withdrawer signs)
    let init_ix = ixn::initialize_checked(
        &stake_acc.pubkey(),
        &Authorized { staker: base.pubkey(), withdrawer: withdrawer.pubkey() },
    );
    let msg = Message::new(&[init_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let new_staker = Keypair::new();
    let ix = ixn::authorize_checked_with_seed(
        &stake_acc.pubkey(),
        &base.pubkey(),
        seed.to_string(),
        &owner,
        &new_staker.pubkey(),
        StakeAuthorize::Staker,
        None,
    );

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

    let new_staker = Keypair::new();
    let ix = ixn::authorize_with_seed(
        &stake_acc.pubkey(),
        &base.pubkey(),
        seed.to_string(),
        &owner,
        &new_staker.pubkey(),
        StakeAuthorize::Staker,
        None,
    );
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

