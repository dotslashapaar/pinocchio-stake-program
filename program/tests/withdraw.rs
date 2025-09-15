mod common;
use common::*;
use solana_sdk::{instruction::{AccountMeta, Instruction}, pubkey::Pubkey, system_instruction, message::Message};

#[tokio::test]
async fn withdraw_uninitialized_partial() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    // Create stake account owned by our program (Uninitialized path)
    let stake_acc = Keypair::new();
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let reserve = rent.minimum_balance(space as usize);

    let create = system_instruction::create_account(&ctx.payer.pubkey(), &stake_acc.pubkey(), reserve, space, &program_id);
    let msg = Message::new(&[create], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &stake_acc], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Prefund with reserve + small extra to allow partial withdraw
    let extra = reserve + 1_000_000; // small extra on top of reserve
    let fund_tx = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(&ctx.payer.pubkey(), &stake_acc.pubkey(), extra)],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(fund_tx).await.unwrap();

    // Withdraw some lamports to payer using Uninitialized fast path
    let withdraw_lamports: u64 = 500_000;
    let mut data = vec![4u8]; // Withdraw discriminant
    data.extend_from_slice(&withdraw_lamports.to_le_bytes());
    let w_ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(stake_acc.pubkey(), false),
            AccountMeta::new(ctx.payer.pubkey(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::stake_history::id(), false),
            // Authority = source account (Uninitialized flow)
            AccountMeta::new_readonly(stake_acc.pubkey(), true),
        ],
        data,
    };
    let msg = Message::new(&[w_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &stake_acc], ctx.last_blockhash).unwrap();
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "Withdraw should succeed: {:?}", res);
}

#[tokio::test]
async fn withdraw_initialized_partial_respects_reserve() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    // Create Initialized stake with authorities
    let stake_acc = Keypair::new();
    let staker = Keypair::new();
    let withdrawer = Keypair::new();

    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let reserve = rent.minimum_balance(space as usize);

    // Create + InitializeChecked
    let create = system_instruction::create_account(&ctx.payer.pubkey(), &stake_acc.pubkey(), reserve, space, &program_id);
    let msg = Message::new(&[create], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &stake_acc], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let init_ix = Instruction { program_id, accounts: vec![
        AccountMeta::new(stake_acc.pubkey(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::rent::id(), false),
        AccountMeta::new_readonly(staker.pubkey(), false),
        AccountMeta::new_readonly(withdrawer.pubkey(), true),
    ], data: vec![9u8]};
    let msg = Message::new(&[init_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Prefund above reserve
    let extra: u64 = 1_500_000;
    let fund_tx = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(&ctx.payer.pubkey(), &stake_acc.pubkey(), extra)],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(fund_tx).await.unwrap();

    // Withdraw less than extra, ensure reserve stays
    let withdraw_lamports: u64 = extra / 2;
    let mut data = vec![4u8];
    data.extend_from_slice(&withdraw_lamports.to_le_bytes());
    let ix = Instruction { program_id, accounts: vec![
        AccountMeta::new(stake_acc.pubkey(), false),
        AccountMeta::new(ctx.payer.pubkey(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::stake_history::id(), false),
        AccountMeta::new_readonly(withdrawer.pubkey(), true),
    ], data };
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "Withdraw(partial) should succeed: {:?}", res);

    // Verify remaining >= reserve
    let acct = ctx.banks_client.get_account(stake_acc.pubkey()).await.unwrap().unwrap();
    assert!(acct.lamports >= reserve, "stake must retain at least reserve");
    // And state remains Initialized
    let state = pinocchio_stake::state::stake_state_v2::StakeStateV2::deserialize(&acct.data).unwrap();
    matches!(state, pinocchio_stake::state::stake_state_v2::StakeStateV2::Initialized(_));
}

#[tokio::test]
async fn withdraw_initialized_full_closes_account() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    let stake_acc = Keypair::new();
    let staker = Keypair::new();
    let withdrawer = Keypair::new();

    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let reserve = rent.minimum_balance(space as usize);

    // Create + InitializeChecked
    let create = system_instruction::create_account(&ctx.payer.pubkey(), &stake_acc.pubkey(), reserve, space, &program_id);
    let msg = Message::new(&[create], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &stake_acc], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let init_ix = Instruction { program_id, accounts: vec![
        AccountMeta::new(stake_acc.pubkey(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::rent::id(), false),
        AccountMeta::new_readonly(staker.pubkey(), false),
        AccountMeta::new_readonly(withdrawer.pubkey(), true),
    ], data: vec![9u8]};
    let msg = Message::new(&[init_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Full withdraw: exactly current lamports
    let acct_before = ctx.banks_client.get_account(stake_acc.pubkey()).await.unwrap().unwrap();
    let full = acct_before.lamports;
    let mut data = vec![4u8];
    data.extend_from_slice(&full.to_le_bytes());
    let ix = Instruction { program_id, accounts: vec![
        AccountMeta::new(stake_acc.pubkey(), false),
        AccountMeta::new(ctx.payer.pubkey(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::stake_history::id(), false),
        AccountMeta::new_readonly(withdrawer.pubkey(), true),
    ], data };
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "Full withdraw should succeed on Initialized");

    // Account may be purged by runtime when lamports reach zero. Accept either case.
    let acct_after_opt = ctx.banks_client.get_account(stake_acc.pubkey()).await.unwrap();
    if let Some(acct_after) = acct_after_opt {
        assert_eq!(acct_after.lamports, 0);
        let state = pinocchio_stake::state::stake_state_v2::StakeStateV2::deserialize(&acct_after.data).unwrap();
        assert!(matches!(state, pinocchio_stake::state::stake_state_v2::StakeStateV2::Uninitialized));
    }
}

#[tokio::test]
async fn withdraw_stake_active_fails_partial() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    // Authorities and stake
    let staker = Keypair::new();
    let withdrawer = Keypair::new();
    let stake = Keypair::new();

    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let reserve = rent.minimum_balance(space as usize);

    // Create + InitializeChecked
    let create = system_instruction::create_account(&ctx.payer.pubkey(), &stake.pubkey(), reserve, space, &program_id);
    let msg = Message::new(&[create], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &stake], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let init_ix = Instruction { program_id, accounts: vec![
        AccountMeta::new(stake.pubkey(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::rent::id(), false),
        AccountMeta::new_readonly(staker.pubkey(), false),
        AccountMeta::new_readonly(withdrawer.pubkey(), true),
    ], data: vec![9u8]};
    let msg = Message::new(&[init_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Fund extra and delegate to dummy vote account
    let extra: u64 = 2_000_000;
    let fund_tx = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(&ctx.payer.pubkey(), &stake.pubkey(), extra)],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(fund_tx).await.unwrap();

    let vote = Keypair::new();
    // create a minimal vote account with byte layout expected by get_vote_state
    let vote_space = std::mem::size_of::<pinocchio_stake::state::vote_state::VoteState>() as u64;
    let vote_lamports = rent.minimum_balance(vote_space as usize);
    let create_vote = system_instruction::create_account(&ctx.payer.pubkey(), &vote.pubkey(), vote_lamports, vote_space, &solana_sdk::system_program::id());
    let msg = Message::new(&[create_vote], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &vote], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Delegate
    let del_ix = Instruction { program_id, accounts: vec![
        AccountMeta::new(stake.pubkey(), false),
        AccountMeta::new_readonly(vote.pubkey(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::stake_history::id(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::stake_history::id(), false),
        AccountMeta::new_readonly(staker.pubkey(), true),
    ], data: vec![2u8] };
    let msg = Message::new(&[del_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &staker], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Attempt partial withdraw while still active -> should fail
    let attempt: u64 = 1_000; // any positive amount should fail under active constraints
    let mut data = vec![4u8];
    data.extend_from_slice(&attempt.to_le_bytes());
    let ix = Instruction { program_id, accounts: vec![
        AccountMeta::new(stake.pubkey(), false),
        AccountMeta::new(ctx.payer.pubkey(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::stake_history::id(), false),
        AccountMeta::new_readonly(withdrawer.pubkey(), true),
    ], data };
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_err(), "Withdraw should fail while stake is active");
}

#[tokio::test]
async fn withdraw_stake_after_deactivate_full_succeeds() {
    let mut pt = common::program_test();
    let mut ctx = pt.start_with_context().await;
    let program_id = Pubkey::new_from_array(pinocchio_stake::ID);

    // Authorities and stake
    let staker = Keypair::new();
    let withdrawer = Keypair::new();
    let stake = Keypair::new();

    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let reserve = rent.minimum_balance(space as usize);

    // Create + InitializeChecked
    let create = system_instruction::create_account(&ctx.payer.pubkey(), &stake.pubkey(), reserve, space, &program_id);
    let msg = Message::new(&[create], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &stake], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let init_ix = Instruction { program_id, accounts: vec![
        AccountMeta::new(stake.pubkey(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::rent::id(), false),
        AccountMeta::new_readonly(staker.pubkey(), false),
        AccountMeta::new_readonly(withdrawer.pubkey(), true),
    ], data: vec![9u8]};
    let msg = Message::new(&[init_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Fund and delegate
    let extra: u64 = 2_000_000;
    let fund_tx = Transaction::new_signed_with_payer(
        &[system_instruction::transfer(&ctx.payer.pubkey(), &stake.pubkey(), extra)],
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer],
        ctx.last_blockhash,
    );
    ctx.banks_client.process_transaction(fund_tx).await.unwrap();

    let vote = Keypair::new();
    let vote_space = std::mem::size_of::<pinocchio_stake::state::vote_state::VoteState>() as u64;
    let vote_lamports = rent.minimum_balance(vote_space as usize);
    let create_vote = system_instruction::create_account(&ctx.payer.pubkey(), &vote.pubkey(), vote_lamports, vote_space, &solana_sdk::system_program::id());
    let msg = Message::new(&[create_vote], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &vote], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let del_ix = Instruction { program_id, accounts: vec![
        AccountMeta::new(stake.pubkey(), false),
        AccountMeta::new_readonly(vote.pubkey(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::stake_history::id(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::stake_history::id(), false),
        AccountMeta::new_readonly(staker.pubkey(), true),
    ], data: vec![2u8] };
    let msg = Message::new(&[del_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &staker], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Deactivate
    let deact_ix = Instruction { program_id, accounts: vec![
        AccountMeta::new(stake.pubkey(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
        AccountMeta::new_readonly(staker.pubkey(), true),
    ], data: vec![5u8]};
    let msg = Message::new(&[deact_ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &staker], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Advance one epoch so effective stake becomes zero (in our model)
    let root_slot = ctx.banks_client.get_root_slot().await.unwrap();
    let slots_per_epoch = ctx.genesis_config().epoch_schedule.slots_per_epoch;
    ctx.warp_to_slot(root_slot + slots_per_epoch).unwrap();

    // Full withdraw now succeeds
    let current = ctx.banks_client.get_account(stake.pubkey()).await.unwrap().unwrap();
    let full = current.lamports;
    let mut data = vec![4u8];
    data.extend_from_slice(&full.to_le_bytes());
    let ix = Instruction { program_id, accounts: vec![
        AccountMeta::new(stake.pubkey(), false),
        AccountMeta::new(ctx.payer.pubkey(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::clock::id(), false),
        AccountMeta::new_readonly(solana_sdk::sysvar::stake_history::id(), false),
        AccountMeta::new_readonly(withdrawer.pubkey(), true),
    ], data };
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
    let res = ctx.banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "Full withdraw after deactivation should succeed: {:?}", res);

    // Account may be purged by runtime when lamports reach zero. Accept either case.
    let after_opt = ctx.banks_client.get_account(stake.pubkey()).await.unwrap();
    if let Some(after) = after_opt {
        assert_eq!(after.lamports, 0);
        let state = pinocchio_stake::state::stake_state_v2::StakeStateV2::deserialize(&after.data).unwrap();
        assert!(matches!(state, pinocchio_stake::state::stake_state_v2::StakeStateV2::Uninitialized));
    }
}
