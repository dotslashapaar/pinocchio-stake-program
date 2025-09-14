mod common;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signer::Signer, transaction::Transaction};

#[tokio::test]
async fn smoke_get_minimum_delegation() {
    // 1) Boot a test bank and load your SBF program via helper
    let mut pt = common::program_test();

    // Use the Stake builtin id from our crate
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    let mut ctx = pt.start_with_context().await;

    // 2) Build the instruction for GetMinimumDelegation (disc=13), no accounts
    let ix = Instruction { program_id, accounts: vec![], data: vec![13u8] };

    // 3) Simulate and read return_data (u64 LE)
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer],
        ctx.last_blockhash,
    );

    let sim = ctx.banks_client.simulate_transaction(tx).await.unwrap();
    let ret = sim
        .simulation_details
        .and_then(|d| d.return_data)
        .expect("program should return data")
        .data;

    let mut buf = [0u8; 8];
    let n = core::cmp::min(ret.len(), 8);
    buf[..n].copy_from_slice(&ret[..n]);
    let minimum = u64::from_le_bytes(buf);

    assert!(minimum >= 1, "minimum delegation should be >= 1, got {}", minimum);
}
