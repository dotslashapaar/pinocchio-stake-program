mod common;
use common::*;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    system_instruction,
};

// SetLockupChecked: when lockup not in force, withdrawer must sign and epoch/timestamp updates apply.
#[tokio::test]
async fn set_lockup_checked_updates_epoch_with_withdrawer_signature() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    // Create stake account owned by our program
    let stake_acc = Keypair::new();
    let staker = Keypair::new();
    let withdrawer = Keypair::new();

    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let reserve = rent.minimum_balance(space as usize);

    let create = system_instruction::create_account(
        &ctx.payer.pubkey(),
        &stake_acc.pubkey(),
        reserve,
        space,
        &program_id,
    );
    let msg = Message::new(&[create], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &stake_acc], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // InitializeChecked: set staker/withdrawer; withdrawer must sign
    let init_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(stake_acc.pubkey(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::rent::id(), false),
            AccountMeta::new_readonly(staker.pubkey(), false),
            AccountMeta::new_readonly(withdrawer.pubkey(), true),
        ],
        data: vec![9u8], // InitializeChecked discriminant
    };
    let msg = Message::new(&[init_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // SetLockupChecked: update only the epoch (flag 0x02)
    let new_epoch: u64 = 5;
    let mut data = vec![12u8]; // SetLockupChecked discriminant
    data.push(0x02); // flags: epoch present
    data.extend_from_slice(&new_epoch.to_le_bytes());

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(stake_acc.pubkey(), false),
            // Include withdrawer as signer in the tx (collect_signers sees it)
            AccountMeta::new_readonly(withdrawer.pubkey(), true),
            // No new custodian passed (index 2) -> custodian unchanged
        ],
        data,
    };
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "SetLockupChecked should succeed: {:?}", res);

    // Verify lockup.epoch updated
    let acct = ctx
        .banks_client
        .get_account(stake_acc.pubkey())
        .await
        .unwrap()
        .expect("stake account must exist");
    let state = pinocchio_stake::state::stake_state_v2::StakeStateV2::deserialize(&acct.data).unwrap();
    match state {
        pinocchio_stake::state::stake_state_v2::StakeStateV2::Initialized(meta)
        | pinocchio_stake::state::stake_state_v2::StakeStateV2::Stake(meta, _, _) => {
            assert_eq!(meta.lockup.epoch, new_epoch);
            // staker/withdrawer unchanged
            assert_eq!(meta.authorized.staker, staker.pubkey().to_bytes());
            assert_eq!(meta.authorized.withdrawer, withdrawer.pubkey().to_bytes());
        }
        other => panic!("unexpected stake state after SetLockupChecked: {:?}", other),
    }
}

// SetLockupChecked: when lockup IS in force, custodian must sign; withdrawer not required.
#[tokio::test]
async fn set_lockup_checked_custodian_in_force() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    // Create stake and authorities
    let stake_acc = Keypair::new();
    let staker = Keypair::new();
    let withdrawer = Keypair::new();
    let custodian = Keypair::new();

    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let reserve = rent.minimum_balance(space as usize);

    let create = system_instruction::create_account(
        &ctx.payer.pubkey(),
        &stake_acc.pubkey(),
        reserve,
        space,
        &program_id,
    );
    let msg = Message::new(&[create], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &stake_acc], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // InitializeChecked
    let init_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(stake_acc.pubkey(), false),
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

    // First, set lockup to be IN FORCE and set custodian (withdrawer signature sufficient when not in force)
    let future_epoch: u64 = ctx.banks_client.get_sysvar::<solana_sdk::clock::Clock>().await.unwrap().epoch + 10;
    let mut data = vec![12u8]; // SetLockupChecked
    data.push(0x02 | 0x00); // only epoch present
    data.extend_from_slice(&future_epoch.to_le_bytes());
    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(stake_acc.pubkey(), false),
            AccountMeta::new_readonly(withdrawer.pubkey(), true), // withdrawer signs (not in force yet)
            AccountMeta::new_readonly(custodian.pubkey(), true),  // set custodian; must be signer if provided
        ],
        data,
    };
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer, &custodian], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Verify lockup set and custodian recorded
    let acct = ctx.banks_client.get_account(stake_acc.pubkey()).await.unwrap().unwrap();
    let state = pinocchio_stake::state::stake_state_v2::StakeStateV2::deserialize(&acct.data).unwrap();
    let (mut meta, in_stake) = match state {
        pinocchio_stake::state::stake_state_v2::StakeStateV2::Initialized(m) => (m, false),
        pinocchio_stake::state::stake_state_v2::StakeStateV2::Stake(m, _, _) => (m, true),
        other => panic!("unexpected state: {:?}", other),
    };
    assert_eq!(meta.lockup.epoch, future_epoch);
    assert_eq!(meta.lockup.custodian, custodian.pubkey().to_bytes());

    // Now lockup is in force -> only custodian signature should be required.
    // Attempt to change unix_timestamp while passing ONLY custodian as signer.
    let new_ts: i64 = 1234567890;
    let mut data2 = vec![12u8];
    data2.push(0x01); // timestamp present
    data2.extend_from_slice(&new_ts.to_le_bytes());
    let ix2 = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(stake_acc.pubkey(), false),
            AccountMeta::new_readonly(custodian.pubkey(), true), // signer set includes custodian
        ],
        data: data2,
    };
    let msg = Message::new(&[ix2], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &custodian], ctx.last_blockhash).unwrap();
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "SetLockupChecked by custodian should succeed: {:?}", res);

    let acct2 = ctx.banks_client.get_account(stake_acc.pubkey()).await.unwrap().unwrap();
    let state2 = pinocchio_stake::state::stake_state_v2::StakeStateV2::deserialize(&acct2.data).unwrap();
    match state2 {
        pinocchio_stake::state::stake_state_v2::StakeStateV2::Initialized(m)
        | pinocchio_stake::state::stake_state_v2::StakeStateV2::Stake(m, _, _) => {
            meta = m;
        }
        other => panic!("unexpected state after custodian update: {:?}", other),
    }
    assert_eq!(meta.lockup.unix_timestamp, new_ts);
}

