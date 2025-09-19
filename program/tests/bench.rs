mod common;
use common::*;
use common::pin_adapter as ixn;

use solana_sdk::{
    instruction::Instruction,
    message::Message,
    signature::Signer,
    stake::state::Authorized,
    system_instruction,
};

async fn simulate(ctx: &mut ProgramTestContext, ixs: &[Instruction], signers: &[&solana_sdk::signature::Keypair]) -> u64 {
    let msg = Message::new(ixs, Some(&ctx.payer.pubkey()));
    let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
    let mut all: Vec<&solana_sdk::signature::Keypair> = Vec::with_capacity(signers.len() + 1);
    all.push(&ctx.payer);
    all.extend_from_slice(signers);
    tx.try_sign(&all, ctx.last_blockhash).unwrap();
    let sim = ctx.banks_client.simulate_transaction(tx).await.unwrap();
    sim.simulation_details.map(|d| d.units_consumed).unwrap_or_default()
}

async fn create_stake_account(ctx: &mut ProgramTestContext, stake: &solana_sdk::signature::Keypair) {
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let lamports = rent.minimum_balance(space as usize);
    let ix = system_instruction::create_account(&ctx.payer.pubkey(), &stake.pubkey(), lamports, space, &solana_sdk::stake::program::id());
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, stake], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();
}

async fn create_vote_like(ctx: &mut ProgramTestContext, kp: &solana_sdk::signature::Keypair) {
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = std::mem::size_of::<pinocchio_stake::state::vote_state::VoteState>() as u64;
    let lamports = rent.minimum_balance(space as usize);
    let ix = system_instruction::create_account(&ctx.payer.pubkey(), &kp.pubkey(), lamports, space, &solana_sdk::vote::program::id());
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, kp], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();
}

#[ignore]
#[tokio::test]
async fn bench_pinocchio_vs_native() {
    // Pinocchio (upgradeable) context
    let mut ctx_pin = program_test().start_with_context().await;
    // Native baseline context
    let mut ctx_nat = program_test_native().start_with_context().await;

    // Stake + authorities
    let stake_a = solana_sdk::signature::Keypair::new();
    let staker = solana_sdk::signature::Keypair::new();
    let withdrawer = solana_sdk::signature::Keypair::new();
    create_stake_account(&mut ctx_pin, &stake_a).await;
    create_stake_account(&mut ctx_nat, &stake_a).await;

    // 1) initialize_checked
    let auth = Authorized { staker: staker.pubkey(), withdrawer: withdrawer.pubkey() };
    let ix_init = ixn::initialize_checked(&stake_a.pubkey(), &auth);
    let units_pin = simulate(&mut ctx_pin, &[ix_init.clone()], &[&withdrawer]).await;
    let units_nat = simulate(&mut ctx_nat, &[ix_init], &[&withdrawer]).await;

    println!("name,pin,native");
    println!("initialize_checked,{units_pin},{units_nat}");

    // 2) delegate (requires prefund + vote)
    // fund stake a bit above reserve
    let extra = 2_000_000u64;
    for ctx in [&mut ctx_pin, &mut ctx_nat] {
        let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
            &[system_instruction::transfer(&ctx.payer.pubkey(), &stake_a.pubkey(), extra)],
            Some(&ctx.payer.pubkey()),
            &[&ctx.payer],
            ctx.last_blockhash,
        );
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }
    let vote = solana_sdk::signature::Keypair::new();
    create_vote_like(&mut ctx_pin, &vote).await;
    create_vote_like(&mut ctx_nat, &vote).await;

    let ix_delegate = ixn::delegate_stake(&stake_a.pubkey(), &staker.pubkey(), &vote.pubkey());
    let units_pin = simulate(&mut ctx_pin, &[ix_delegate.clone()], &[&staker]).await;
    let units_nat = simulate(&mut ctx_nat, &[ix_delegate], &[&staker]).await;
    println!("delegate,{units_pin},{units_nat}");
}
