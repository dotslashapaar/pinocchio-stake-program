mod common;
use common::*;
use common::pin_adapter as ixn;
use solana_sdk::stake::state::Authorized;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::{system_instruction, message::Message};

#[tokio::test]
async fn authorize_harness_boots() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    let ix = ixn::get_minimum_delegation();
    let tx = Transaction::new_signed_with_payer(&[ix], Some(&ctx.payer.pubkey()), &[&ctx.payer], ctx.last_blockhash);
    let sim = ctx.banks_client.simulate_transaction(tx).await.unwrap();
    assert!(sim.simulation_details.unwrap().return_data.is_some());
}

#[tokio::test]
async fn authorize_checked_staker_success() {
    // Build context
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    // Create stake account owned by our program, rent-exempt and correct size
    let stake_account = Keypair::new();
    let staker = Keypair::new();
    let withdrawer = Keypair::new();

    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let lamports = rent.minimum_balance(space as usize);
    let create_ix = system_instruction::create_account(
        &ctx.payer.pubkey(),
        &stake_account.pubkey(),
        lamports,
        space,
        &program_id,
    );
    let msg = Message::new(&[create_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &stake_account], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // InitializeChecked via adapter: withdrawer must sign; staker provided as account
    let auth = Authorized { staker: staker.pubkey(), withdrawer: withdrawer.pubkey() };
    let init_ix = ixn::initialize_checked(&stake_account.pubkey(), &auth);
    let msg = Message::new(&[init_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // AuthorizeChecked for Staker (role=0). Old staker and new staker must sign.
    let new_staker = Keypair::new();
    let auth_ix = ixn::authorize_checked(
        &stake_account.pubkey(),
        &staker.pubkey(),
        &new_staker.pubkey(),
        solana_sdk::stake::state::StakeAuthorize::Staker,
        None,
    );
    let msg = Message::new(&[auth_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &staker, &new_staker], ctx.last_blockhash).unwrap();
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "AuthorizeChecked(Staker) should succeed: {:?}", res);

    // Verify staker changed
    let acct = ctx
        .banks_client
        .get_account(stake_account.pubkey())
        .await
        .unwrap()
        .expect("stake account must exist");
    let state = pinocchio_stake::state::stake_state_v2::StakeStateV2::deserialize(&acct.data).unwrap();
    match state {
        pinocchio_stake::state::stake_state_v2::StakeStateV2::Initialized(meta) => {
            assert_eq!(meta.authorized.staker, new_staker.pubkey().to_bytes());
            assert_eq!(meta.authorized.withdrawer, withdrawer.pubkey().to_bytes());
        }
        pinocchio_stake::state::stake_state_v2::StakeStateV2::Stake(meta, _stake, _flags) => {
            assert_eq!(meta.authorized.staker, new_staker.pubkey().to_bytes());
            assert_eq!(meta.authorized.withdrawer, withdrawer.pubkey().to_bytes());
        }
        other => panic!("expected Initialized/Stake, got {:?}", other),
    }
}
