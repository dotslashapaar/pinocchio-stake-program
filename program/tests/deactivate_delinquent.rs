
mod common;
use common::*;
use solana_sdk::{
    account::Account as SolanaAccount,
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    system_instruction,
};

fn build_epoch_credits_bytes(list: &[(u64, u64, u64)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + list.len() * 24);
    out.extend_from_slice(&(list.len() as u32).to_le_bytes());
    for &(e, c, p) in list {
        out.extend_from_slice(&e.to_le_bytes());
        out.extend_from_slice(&c.to_le_bytes());
        out.extend_from_slice(&p.to_le_bytes());
    }
    out
}

#[cfg(feature = "e2e")]
#[tokio::test]
async fn deactivate_delinquent_happy_path() {
    // Prepare vote accounts at genesis with fixed epoch credits
    let mut pt = common::program_test();

    // Choose target current epoch = 5 to satisfy N=5 requirements
    // Reference vote must have last 5 epochs exactly [5,4,3,2,1]
    let reference_votes = build_epoch_credits_bytes(&[(1, 1, 0), (2, 1, 0), (3, 1, 0), (4, 1, 0), (5, 1, 0)]);
    // Delinquent vote last vote epoch = 0 (older than current-5 => eligible)
    let delinquent_votes = build_epoch_credits_bytes(&[(0, 1, 0)]);

    let reference_vote = Pubkey::new_unique();
    let delinquent_vote = Pubkey::new_unique();

    // Add accounts to test genesis (owner doesn't matter; program only reads bytes)
    pt.add_account(
        reference_vote,
        SolanaAccount {
            lamports: 1_000_000,
            data: reference_votes,
            owner: solana_sdk::system_program::id(),
            executable: false,
            rent_epoch: 0,
        },
    );
    pt.add_account(
        delinquent_vote,
        SolanaAccount {
            lamports: 1_000_000,
            data: delinquent_votes,
            owner: solana_sdk::system_program::id(),
            executable: false,
            rent_epoch: 0,
        },
    );

    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    // Warp to epoch 5 so that reference sequence [1..5] matches and min_epoch = 0
    let slots_per_epoch = ctx.genesis_config().epoch_schedule.slots_per_epoch;
    let first_normal = ctx.genesis_config().epoch_schedule.first_normal_slot;
    let target_slot = first_normal + slots_per_epoch * 5 + 1;
    ctx.warp_to_slot(target_slot).unwrap();

    // Create stake account and initialize authorities
    let staker = Keypair::new();
    let withdrawer = Keypair::new();
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

    // Prefund above reserve to delegate non-zero stake
    let extra: u64 = 2_000_000;
    let fund_tx = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(&ctx.payer.pubkey(), &stake.pubkey(), extra)],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(fund_tx).await.unwrap();

    // Delegate to the delinquent vote account (staker signs)
    let del_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(stake.pubkey(), false),
            AccountMeta::new_readonly(delinquent_vote, false),
            AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::stake_history::id(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::stake_history::id(), false),
            AccountMeta::new_readonly(staker.pubkey(), true),
        ],
        data: vec![2u8],
    };
    let msg = Message::new(&[del_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &staker], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Now call DeactivateDelinquent: [stake, delinquent_vote, reference_vote]
    let dd_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(stake.pubkey(), false),
            AccountMeta::new_readonly(delinquent_vote, false),
            AccountMeta::new_readonly(reference_vote, false),
        ],
        data: vec![14u8],
    };
    let msg = Message::new(&[dd_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    // No signer required by this instruction
    tx.try_sign(&[&ctx.payer], ctx.last_blockhash).unwrap();
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "DeactivateDelinquent should succeed: {:?}", res);

    // Verify stake got deactivated at current epoch
    let clock = ctx.banks_client.get_sysvar::<solana_sdk::clock::Clock>().await.unwrap();
    let acct = ctx.banks_client.get_account(stake.pubkey()).await.unwrap().unwrap();
    let state = pinocchio_stake::state::stake_state_v2::StakeStateV2::deserialize(&acct.data).unwrap();
    match state {
        pinocchio_stake::state::stake_state_v2::StakeStateV2::Stake(_meta, stake_data, _flags) => {
            let deact = u64::from_le_bytes(stake_data.delegation.deactivation_epoch);
            assert_eq!(deact, clock.epoch);
        }
        other => panic!("expected Stake state, got {:?}", other),
    }
}
