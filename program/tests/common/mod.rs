use solana_program_test::{ProgramTest, ProgramTestBanksClientExt};
use std::{env, path::Path};

pub use solana_program_test::{BanksClient, ProgramTestContext};
pub use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    signers::Signers,
    transaction::Transaction,
    system_instruction,
};

pub fn program_test() -> ProgramTest {
    program_test_without_features(&[])
}

pub fn program_test_without_features(feature_ids: &[Pubkey]) -> ProgramTest {
    let deploy_dir = format!("{}/target/deploy", env!("CARGO_MANIFEST_DIR"));
    env::set_var("BPF_OUT_DIR", &deploy_dir);
    let so_path = Path::new(&deploy_dir).join("pinocchio_stake.so");
    assert!(
        so_path.exists(),
        "SBF artifact not found at {}.\nBuild first: `cargo-build-sbf --no-default-features --features sbf --manifest-path program/Cargo.toml`",
        so_path.display()
    );

    let mut pt = ProgramTest::default();
    pt.prefer_bpf(true);
    // Allow headroom for heavier flows while debugging
    pt.set_compute_max_units(1_000_000);
    for feature in feature_ids {
        pt.deactivate_feature(*feature);
    }
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);
    pt.add_upgradeable_program_to_genesis("pinocchio_stake", &program_id);
    pt
}

// Shared adapter for instruction translation + state helpers
pub mod pin_adapter;

pub async fn refresh_blockhash(ctx: &mut ProgramTestContext) {
    ctx.last_blockhash = ctx
        .banks_client
        .get_new_latest_blockhash(&ctx.last_blockhash)
        .await
        .unwrap();
}

pub async fn transfer(ctx: &mut ProgramTestContext, recipient: &Pubkey, amount: u64) {
    let tx = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(&ctx.payer.pubkey(), recipient, amount)],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(tx).await.unwrap();
}

// Native baseline: do not override the builtin Stake program
pub fn program_test_native() -> ProgramTest {
    let mut pt = ProgramTest::default();
    pt.prefer_bpf(true);
    pt.set_compute_max_units(1_000_000);
    pt
}
