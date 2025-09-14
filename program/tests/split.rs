mod common;
use common::*;
use solana_sdk::{instruction::{AccountMeta, Instruction}, pubkey::Pubkey, system_instruction, message::Message};

#[tokio::test]
async fn split_from_initialized_into_uninitialized() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    // Prepare source stake: rent-exempt + extra lamports for split; Initialized with staker/withdrawer.
    let source = Keypair::new();
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space_src = pinocchio_stake::state::stake_state_v2::StakeStateV2::size_of() as u64;
    let space_dest: u64 = 4096; // generous to avoid layout discrepancies
    let reserve = rent.minimum_balance(space_src as usize);

    // Create source account owned by program with only reserve lamports
    let create_src = system_instruction::create_account(
        &ctx.payer.pubkey(), &source.pubkey(), reserve, space_src, &program_id,
    );
    let msg = Message::new(&[create_src], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &source], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Prefund source with split amount (so source has funds to move while Uninitialized)
    let split_lamports = rent.minimum_balance(space_src as usize);
    let fund_tx = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(&ctx.payer.pubkey(), &source.pubkey(), split_lamports)],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(fund_tx).await.unwrap();

    // Prepare destination: uninitialized stake account, prefunded to rent-exempt for its data size
    let dest = Keypair::new();
    let dest_rent = rent.minimum_balance(space_dest as usize);
    let create_dest = system_instruction::create_account(
        &ctx.payer.pubkey(), &dest.pubkey(), dest_rent, space_dest, &program_id,
    );
    let msg = Message::new(&[create_dest], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &dest], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Debug destination after creation
    let dest_acc = ctx.banks_client.get_account(dest.pubkey()).await.unwrap().unwrap();
    eprintln!("test debug: dest owner={} expected={}, data_len={} space_dest={}", dest_acc.owner, program_id, dest_acc.data.len(), space_dest);

    // Split: source (writable signer), destination (writable), third account unused
    let mut data = vec![3u8]; // Split discriminant
    data.extend_from_slice(&(split_lamports as u64).to_le_bytes());
    let split_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(source.pubkey(), true),
            AccountMeta::new(dest.pubkey(), false),
            AccountMeta::new_readonly(source.pubkey(), true),
        ],
        data,
    };
    let msg = Message::new(&[split_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &source], ctx.last_blockhash).unwrap();
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "Split should succeed: {:?}", res);
}
