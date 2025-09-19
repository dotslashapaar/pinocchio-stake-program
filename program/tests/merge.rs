mod common;
use common::*;
use common::pin_adapter as ixn;
use solana_sdk::{
    message::Message,
    pubkey::Pubkey,
    system_instruction,
    stake::state::Authorized,
};

async fn create_initialized_stake(
    ctx: &mut ProgramTestContext,
    program_id: &Pubkey,
    staker: &Keypair,
    withdrawer: &Keypair,
    extra_lamports: u64,
) -> Keypair {
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let reserve = rent.minimum_balance(space as usize);
    let kp = Keypair::new();

    // Create account owned by program
    let create = system_instruction::create_account(&ctx.payer.pubkey(), &kp.pubkey(), reserve, space, program_id);
    let msg = Message::new(&[create], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &kp], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // InitializeChecked via adapter
    let init_ix = ixn::initialize_checked(
        &kp.pubkey(),
        &Authorized { staker: staker.pubkey(), withdrawer: withdrawer.pubkey() },
    );
    let msg = Message::new(&[init_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, withdrawer], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Prefund if requested
    if extra_lamports > 0 {
        let fund = Transaction::new_signed_with_payer(
            &[system_instruction::transfer(&ctx.payer.pubkey(), &kp.pubkey(), extra_lamports)],
            Some(&ctx.payer.pubkey()),
            &[&ctx.payer],
            ctx.last_blockhash,
        );
        ctx.banks_client.process_transaction(fund).await.unwrap();
    }

    kp
}

#[tokio::test]
async fn merge_inactive_into_inactive_succeeds_and_drains_source() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    let staker = Keypair::new();
    let withdrawer = Keypair::new();

    let dst = create_initialized_stake(&mut ctx, &program_id, &staker, &withdrawer, 1_000_000).await;
    let src = create_initialized_stake(&mut ctx, &program_id, &staker, &withdrawer, 500_000).await;

    let dst_before = ctx.banks_client.get_account(dst.pubkey()).await.unwrap().unwrap();
    let src_before = ctx.banks_client.get_account(src.pubkey()).await.unwrap().unwrap();

    // Merge: [dst, src, clock, stake_history, staker signer]
    let ix = ixn::merge(&dst.pubkey(), &src.pubkey(), &staker.pubkey())
        .into_iter()
        .next()
        .unwrap();
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &staker], ctx.last_blockhash).unwrap();
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "Merge should succeed for inactive+inactive: {:?}", res);

    let dst_after = ctx.banks_client.get_account(dst.pubkey()).await.unwrap().unwrap();
    let src_after_opt = ctx.banks_client.get_account(src.pubkey()).await.unwrap();

    // Destination lamports increase by source lamports
    assert_eq!(dst_before.lamports + src_before.lamports, dst_after.lamports);
    // Source drained; it may be deleted by runtime if lamports==0, or present with 0 lamports
    if let Some(src_after) = src_after_opt {
        assert_eq!(src_after.lamports, 0);
        let src_state = pinocchio_stake::state::stake_state_v2::StakeStateV2::deserialize(&src_after.data).unwrap();
        assert!(matches!(src_state, pinocchio_stake::state::stake_state_v2::StakeStateV2::Uninitialized));
    }
}

#[tokio::test]
async fn merge_missing_staker_signature_fails() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    let staker = Keypair::new();
    let withdrawer = Keypair::new();
    let dst = create_initialized_stake(&mut ctx, &program_id, &staker, &withdrawer, 0).await;
    let src = create_initialized_stake(&mut ctx, &program_id, &staker, &withdrawer, 0).await;

    let mut ix = ixn::merge(&dst.pubkey(), &src.pubkey(), &staker.pubkey())
        .into_iter()
        .next()
        .unwrap();
    // remove staker signer to assert signature failure path is handled
    ix.accounts.retain(|am| am.pubkey != staker.pubkey());
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer], ctx.last_blockhash).unwrap();
    let err = ctx.banks_client.process_transaction(tx).await.unwrap_err();
    match err {
        solana_program_test::BanksClientError::TransactionError(te) => {
            use solana_sdk::transaction::TransactionError;
            assert!(matches!(te, TransactionError::InstructionError(_, _)));
        }
        other => panic!("unexpected banks client error: {:?}", other),
    }
}

#[tokio::test]
async fn merge_authority_mismatch_fails() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    // Different authorities to force metas_can_merge failure
    let staker_a = Keypair::new();
    let withdrawer_a = Keypair::new();
    let staker_b = Keypair::new();
    let withdrawer_b = Keypair::new();

    let dst = create_initialized_stake(&mut ctx, &program_id, &staker_a, &withdrawer_a, 0).await;
    let src = create_initialized_stake(&mut ctx, &program_id, &staker_b, &withdrawer_b, 0).await;

    let ix = ixn::merge(&dst.pubkey(), &src.pubkey(), &staker_a.pubkey())
        .into_iter()
        .next()
        .unwrap();
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &staker_a], ctx.last_blockhash).unwrap();
    let err = ctx.banks_client.process_transaction(tx).await.unwrap_err();
    match err {
        solana_program_test::BanksClientError::TransactionError(te) => {
            use solana_sdk::transaction::TransactionError;
            assert!(matches!(te, TransactionError::InstructionError(_, _)));
        }
        other => panic!("unexpected banks client error: {:?}", other),
    }
}
