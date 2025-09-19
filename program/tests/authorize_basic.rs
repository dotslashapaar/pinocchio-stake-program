mod common;
use common::*;
use common::pin_adapter as ixn;
use solana_sdk::{
    instruction::AccountMeta,
    message::Message,
    pubkey::Pubkey,
    system_instruction,
    stake::state::{Authorized, StakeAuthorize},
};

#[tokio::test]
async fn authorize_nonchecked_staker_success() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    // Create and initialize stake account with initial authorities
    let stake = Keypair::new();
    let staker = Keypair::new();
    let withdrawer = Keypair::new();

    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let reserve = rent.minimum_balance(space as usize);
    let create = system_instruction::create_account(&ctx.payer.pubkey(), &stake.pubkey(), reserve, space, &program_id);
    let msg = Message::new(&[create], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &stake], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let init_ix = ixn::initialize_checked(
        &stake.pubkey(),
        &Authorized { staker: staker.pubkey(), withdrawer: withdrawer.pubkey() },
    );
    let msg = Message::new(&[init_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Authorize to new staker; only old staker must sign
    let new_staker = Keypair::new();
    let ix = ixn::authorize(
        &stake.pubkey(),
        &staker.pubkey(),
        &new_staker.pubkey(),
        StakeAuthorize::Staker,
        None,
    );
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &staker], ctx.last_blockhash).unwrap();
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "Authorize(Staker) should succeed: {:?}", res);

    // Verify
    let acct = ctx.banks_client.get_account(stake.pubkey()).await.unwrap().unwrap();
    let state = pinocchio_stake::state::stake_state_v2::StakeStateV2::deserialize(&acct.data).unwrap();
    match state { pinocchio_stake::state::stake_state_v2::StakeStateV2::Initialized(meta)
        | pinocchio_stake::state::stake_state_v2::StakeStateV2::Stake(meta, _, _) => {
            assert_eq!(meta.authorized.staker, new_staker.pubkey().to_bytes());
            assert_eq!(meta.authorized.withdrawer, withdrawer.pubkey().to_bytes());
        }
        other => panic!("unexpected state: {:?}", other)
    }
}

#[tokio::test]
async fn authorize_nonchecked_withdrawer_success() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    let stake = Keypair::new();
    let staker = Keypair::new();
    let withdrawer = Keypair::new();

    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let reserve = rent.minimum_balance(space as usize);
    let create = system_instruction::create_account(&ctx.payer.pubkey(), &stake.pubkey(), reserve, space, &program_id);
    let msg = Message::new(&[create], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &stake], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let init_ix = ixn::initialize_checked(
        &stake.pubkey(),
        &Authorized { staker: staker.pubkey(), withdrawer: withdrawer.pubkey() },
    );
    let msg = Message::new(&[init_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let new_withdrawer = Keypair::new();
    let mut ix = ixn::authorize(
        &stake.pubkey(),
        &withdrawer.pubkey(),
        &new_withdrawer.pubkey(),
        StakeAuthorize::Withdrawer,
        None,
    );
    // Simulate missing old-authority signature by removing it from metas
    ix.accounts.retain(|am| am.pubkey != withdrawer.pubkey());
    // Ensure withdrawer appears as a signer meta (some SDK builders can omit when reordered)
    let mut ix = ix;
    if let Some(pos) = ix.accounts.iter().position(|am| am.pubkey == withdrawer.pubkey()) {
        ix.accounts[pos].is_signer = true;
    } else {
        ix.accounts.push(AccountMeta::new_readonly(withdrawer.pubkey(), true));
    }
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer, &withdrawer],
        ctx.last_blockhash,
    );
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "Authorize(Withdrawer) should succeed: {:?}", res);

    let acct = ctx.banks_client.get_account(stake.pubkey()).await.unwrap().unwrap();
    let state = pinocchio_stake::state::stake_state_v2::StakeStateV2::deserialize(&acct.data).unwrap();
    match state { pinocchio_stake::state::stake_state_v2::StakeStateV2::Initialized(meta)
        | pinocchio_stake::state::stake_state_v2::StakeStateV2::Stake(meta, _, _) => {
            assert_eq!(meta.authorized.staker, staker.pubkey().to_bytes());
            assert_eq!(meta.authorized.withdrawer, new_withdrawer.pubkey().to_bytes());
        }
        other => panic!("unexpected state: {:?}", other)
    }
}

#[tokio::test]
async fn authorize_nonchecked_missing_old_signer_fails() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    let stake = Keypair::new();
    let staker = Keypair::new();
    let withdrawer = Keypair::new();

    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let reserve = rent.minimum_balance(space as usize);
    let create = system_instruction::create_account(&ctx.payer.pubkey(), &stake.pubkey(), reserve, space, &program_id);
    let msg = Message::new(&[create], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &stake], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let init_ix = ixn::initialize_checked(
        &stake.pubkey(),
        &Authorized { staker: staker.pubkey(), withdrawer: withdrawer.pubkey() },
    );
    let msg = Message::new(&[init_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Attempt to change withdrawer but do NOT include current withdrawer signer
    let new_withdrawer = Keypair::new();
    let mut ix = ixn::authorize(
        &stake.pubkey(),
        &withdrawer.pubkey(),
        &new_withdrawer.pubkey(),
        StakeAuthorize::Withdrawer,
        None,
    );
    // Remove all signer flags to simulate missing old-authority signature
    ix.accounts.iter_mut().for_each(|am| am.is_signer = false);
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer], ctx.last_blockhash).unwrap();
    let err = ctx.banks_client.process_transaction(tx).await.unwrap_err();
    match err {
        solana_program_test::BanksClientError::TransactionError(te) => {
            use solana_sdk::instruction::InstructionError;
            use solana_sdk::transaction::TransactionError;
            match te {
                TransactionError::InstructionError(_, InstructionError::MissingRequiredSignature) => {}
                TransactionError::InstructionError(_, InstructionError::Custom(_)) => {}
                other => panic!("unexpected error: {:?}", other),
            }
        }
        other => panic!("unexpected banks client error: {:?}", other),
    }
}
