mod common;
use common::*;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;

#[tokio::test]
async fn initialize_harness_boots() {
    // Sanity: ensure our ProgramTest loads the SBF and can execute a simple query
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;

    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);
    let ix = Instruction { program_id, accounts: vec![], data: vec![13u8] };

    let tx = Transaction::new_signed_with_payer(&[ix], Some(&ctx.payer.pubkey()), &[&ctx.payer], ctx.last_blockhash);
    let sim = ctx.banks_client.simulate_transaction(tx).await.unwrap();
    assert!(sim.simulation_details.unwrap().return_data.is_some());
}

// Additional initialize flow tests will be added here after wiring required accounts
