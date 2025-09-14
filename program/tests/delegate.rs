mod common;
use common::*;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    system_instruction,
};

fn vote_state_space() -> u64 {
    std::mem::size_of::<pinocchio_stake::state::vote_state::VoteState>() as u64
}

async fn create_dummy_vote_account(ctx: &mut ProgramTestContext, kp: &Keypair) {
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = vote_state_space();
    let lamports = rent.minimum_balance(space as usize);
    let ix = system_instruction::create_account(
        &ctx.payer.pubkey(),
        &kp.pubkey(),
        lamports,
        space,
        &solana_sdk::system_program::id(),
    );
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, kp], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();
}

#[tokio::test]
async fn delegate_stake_success_sets_state_and_amount() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    // Authorities
    let staker = Keypair::new();
    let withdrawer = Keypair::new();

    // Stake account
    let stake = Keypair::new();
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let reserve = rent.minimum_balance(space as usize);
    let create_stake = system_instruction::create_account(
        &ctx.payer.pubkey(),
        &stake.pubkey(),
        reserve,
        space,
        &program_id,
    );
    let msg = Message::new(&[create_stake], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &stake], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // InitializeChecked
    let init_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(stake.pubkey(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::rent::id(), false),
            AccountMeta::new_readonly(staker.pubkey(), false),
            AccountMeta::new_readonly(withdrawer.pubkey(), true),
        ],
        data: vec![9u8],
    };
    let msg = Message::new(&[init_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Prefund above reserve to create positive stake amount
    let extra: u64 = 2_000_000;
    let fund_tx = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(
            &ctx.payer.pubkey(),
            &stake.pubkey(),
            extra,
        )],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(fund_tx).await.unwrap();

    // Dummy vote
    let vote_acc = Keypair::new();
    create_dummy_vote_account(&mut ctx, &vote_acc).await;

    // Delegate
    let del_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(stake.pubkey(), false),
            AccountMeta::new_readonly(vote_acc.pubkey(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::stake_history::id(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::stake_history::id(), false), // stake_config placeholder
            AccountMeta::new_readonly(staker.pubkey(), true),
        ],
        data: vec![2u8],
    };
    let msg = Message::new(&[del_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &staker], ctx.last_blockhash).unwrap();
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "DelegateStake should succeed: {:?}", res);

    // Verify stake state and amounts
    let clock = ctx.banks_client.get_sysvar::<solana_sdk::clock::Clock>().await.unwrap();
    let acct = ctx.banks_client.get_account(stake.pubkey()).await.unwrap().unwrap();
    let state = pinocchio_stake::state::stake_state_v2::StakeStateV2::deserialize(&acct.data).unwrap();
    match state {
        pinocchio_stake::state::stake_state_v2::StakeStateV2::Stake(meta, stake_data, _flags) => {
            let delegated = u64::from_le_bytes(stake_data.delegation.stake);
            assert_eq!(delegated, extra, "delegated stake equals extra lamports above reserve");
            assert_eq!(stake_data.delegation.voter_pubkey, vote_acc.pubkey().to_bytes());
            assert_eq!(u64::from_le_bytes(stake_data.delegation.activation_epoch), clock.epoch);
            assert_eq!(u64::from_le_bytes(stake_data.delegation.deactivation_epoch), u64::MAX);
            // Sanity: meta.authorized unchanged
            assert_eq!(meta.authorized.staker, staker.pubkey().to_bytes());
            assert_eq!(meta.authorized.withdrawer, withdrawer.pubkey().to_bytes());
        }
        other => panic!("expected Stake state, got {:?}", other),
    }
}

