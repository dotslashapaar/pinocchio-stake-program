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
use solana_sdk::stake::instruction as sdk_stake_ixn;

async fn simulate(ctx: &mut ProgramTestContext, ixs: &[Instruction], signers: &[&solana_sdk::signature::Keypair]) -> u64 {
    let msg = Message::new(ixs, Some(&ctx.payer.pubkey()));
    let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
    let mut all: Vec<&solana_sdk::signature::Keypair> = Vec::with_capacity(signers.len() + 1);
    all.push(&ctx.payer);
    all.extend_from_slice(signers);
    tx.try_sign(&all, ctx.last_blockhash).unwrap();
    let sim = ctx.banks_client.simulate_transaction(tx).await.unwrap();
    if let Some(Err(err)) = sim.result {
        eprintln!("simulation error: {:?}", err);
        if let Some(details) = sim.simulation_details.as_ref() {
            for l in &details.logs { eprintln!("log: {}", l); }
        }
        panic!("simulation failed");
    }
    sim.simulation_details.map(|d| d.units_consumed).unwrap_or_default()
}

async fn create_stake_account_pin(ctx: &mut ProgramTestContext, stake: &solana_sdk::signature::Keypair) {
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = pinocchio_stake::state::stake_state_v2::StakeStateV2::ACCOUNT_SIZE as u64;
    let lamports = rent.minimum_balance(space as usize);
    let program_id = solana_sdk::pubkey::Pubkey::new_from_array(pinocchio_stake::ID);
    let ix = system_instruction::create_account(&ctx.payer.pubkey(), &stake.pubkey(), lamports, space, &program_id);
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, stake], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();
}

async fn create_stake_account_native(ctx: &mut ProgramTestContext, stake: &solana_sdk::signature::Keypair) {
    let rent = ctx.banks_client.get_rent().await.unwrap();
    // Use native stake account size for native context
    let space = solana_stake_program::stake_state::StakeStateV2::size_of() as u64;
    let lamports = rent.minimum_balance(space as usize);
    let ix = system_instruction::create_account(&ctx.payer.pubkey(), &stake.pubkey(), lamports, space, &solana_sdk::stake::program::id());
    let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
    let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
    tx.try_sign(&[&ctx.payer, stake], ctx.last_blockhash).unwrap();
    ctx.banks_client.process_transaction(tx).await.unwrap();
}

// Create and initialize a real vote account via the vote program so that
// delegate has a valid target and exercises realistic code paths.
async fn create_vote_account(
    ctx: &mut ProgramTestContext,
    vote: &solana_sdk::signature::Keypair,
    node: &solana_sdk::signature::Keypair,
) {
    use solana_sdk::vote::{instruction as vote_ixn, state::{VoteInit, VoteStateV3}};

    let rent = ctx.banks_client.get_rent().await.unwrap();
    let rent_voter = rent.minimum_balance(VoteStateV3::size_of());

    let mut ixs = vec![
        // Node/validator must exist and sign
        system_instruction::create_account(
            &ctx.payer.pubkey(),
            &node.pubkey(),
            rent.minimum_balance(0),
            0,
            &solana_sdk::system_program::id(),
        ),
    ];
    ixs.append(&mut vote_ixn::create_account_with_config(
        &ctx.payer.pubkey(),
        &vote.pubkey(),
        &VoteInit {
            node_pubkey: node.pubkey(),
            authorized_voter: node.pubkey(),
            authorized_withdrawer: ctx.payer.pubkey(),
            commission: 0,
        },
        rent_voter,
        solana_sdk::vote::instruction::CreateVoteAccountConfig {
            space: VoteStateV3::size_of() as u64,
            ..Default::default()
        },
    ));

    let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
        &ixs,
        Some(&ctx.payer.pubkey()),
        &[&ctx.payer, vote, node],
        ctx.last_blockhash,
    );
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
    create_stake_account_pin(&mut ctx_pin, &stake_a).await;
    create_stake_account_native(&mut ctx_nat, &stake_a).await;

    // 1) initialize_checked
    let auth = Authorized { staker: staker.pubkey(), withdrawer: withdrawer.pubkey() };
    let ix_init_pin = ixn::initialize_checked(&stake_a.pubkey(), &auth);
    let ix_init_nat = sdk_stake_ixn::initialize_checked(&stake_a.pubkey(), &auth);
    let units_pin = simulate(&mut ctx_pin, &[ix_init_pin.clone()], &[&withdrawer]).await;
    let units_nat = simulate(&mut ctx_nat, &[ix_init_nat.clone()], &[&withdrawer]).await;

    println!("name,pin,native");
    println!("initialize_checked,{units_pin},{units_nat}");
    // Apply initialize so subsequent delegate sees Initialized state
    for (ctx, ix) in [(&mut ctx_pin, ix_init_pin), (&mut ctx_nat, ix_init_nat)] {
        let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
        let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
        tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }

    // 1a) authorize_checked (change withdrawer)
    let new_withdrawer = solana_sdk::signature::Keypair::new();
    let ix_auth_pin = ixn::authorize_checked(
        &stake_a.pubkey(),
        &withdrawer.pubkey(),
        &new_withdrawer.pubkey(),
        solana_sdk::stake::state::StakeAuthorize::Withdrawer,
        None,
    );
    let ix_auth_nat = sdk_stake_ixn::authorize_checked(
        &stake_a.pubkey(),
        &withdrawer.pubkey(),
        &new_withdrawer.pubkey(),
        solana_sdk::stake::state::StakeAuthorize::Withdrawer,
        None,
    );
    // authorize_checked requires the current authority AND the new authorized
    // signer to both sign
    let units_pin = simulate(&mut ctx_pin, &[ix_auth_pin.clone()], &[&withdrawer, &new_withdrawer]).await;
    let units_nat = simulate(&mut ctx_nat, &[ix_auth_nat.clone()], &[&withdrawer, &new_withdrawer]).await;
    println!("authorize_checked,{units_pin},{units_nat}");

    // Apply authorize_checked so subsequent lockup_checked can be signed by the new withdrawer
    for (ctx, ix) in [(&mut ctx_pin, ix_auth_pin), (&mut ctx_nat, ix_auth_nat)] {
        let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
        let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
        tx.try_sign(&[&ctx.payer, &withdrawer, &new_withdrawer], ctx.last_blockhash).unwrap();
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }

    // 1b) set_lockup_checked
    let args = solana_sdk::stake::instruction::LockupArgs { unix_timestamp: Some(0), epoch: None, custodian: None };
    let ix_lock_pin = ixn::set_lockup_checked(&stake_a.pubkey(), &args, &new_withdrawer.pubkey());
    let ix_lock_nat = solana_sdk::stake::instruction::set_lockup_checked(&stake_a.pubkey(), &args, &new_withdrawer.pubkey());
    let units_pin = simulate(&mut ctx_pin, &[ix_lock_pin], &[&new_withdrawer]).await;
    let units_nat = simulate(&mut ctx_nat, &[ix_lock_nat], &[&new_withdrawer]).await;
    println!("set_lockup_checked,{units_pin},{units_nat}");

    // 2) delegate (requires prefund + vote)
    // fund stake a bit above reserve
    let extra = 2_000_000_000u64; // 2 SOL to satisfy native min delegation
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
    let node = solana_sdk::signature::Keypair::new();
    create_vote_account(&mut ctx_pin, &vote, &node).await;
    create_vote_account(&mut ctx_nat, &vote, &node).await;

    // Sanity: read Pinocchio stake account and check state before delegate
    if let Some(acct) = ctx_pin.banks_client.get_account(stake_a.pubkey()).await.unwrap() {
        eprintln!("pin stake owner: {} len={} lamports={}", acct.owner, acct.data.len(), acct.lamports);
        if let Ok(st) = pinocchio_stake::state::stake_state_v2::StakeStateV2::deserialize(&acct.data) {
            eprintln!("pin stake state discriminant ok: {:?}", match st { pinocchio_stake::state::stake_state_v2::StakeStateV2::Initialized(_) => "Initialized", pinocchio_stake::state::stake_state_v2::StakeStateV2::Stake(_,_,_) => "Stake", pinocchio_stake::state::stake_state_v2::StakeStateV2::Uninitialized => "Uninit", _ => "Other" });
        } else {
            eprintln!("pin stake state: deserialize FAILED");
        }
    }

    let ix_delegate_pin = ixn::delegate_stake(&stake_a.pubkey(), &staker.pubkey(), &vote.pubkey());
    let ix_delegate_nat = sdk_stake_ixn::delegate_stake(&stake_a.pubkey(), &staker.pubkey(), &vote.pubkey());
    eprintln!("pin ix accounts (order):");
    for (i, am) in ix_delegate_pin.accounts.iter().enumerate() { eprintln!("  {}: {} w={} s={}", i, am.pubkey, am.is_writable, am.is_signer); }
    eprintln!("nat ix accounts (order):");
    for (i, am) in ix_delegate_nat.accounts.iter().enumerate() { eprintln!("  {}: {} w={} s={}", i, am.pubkey, am.is_writable, am.is_signer); }
    let units_pin = simulate(&mut ctx_pin, &[ix_delegate_pin], &[&staker]).await;
    let units_nat = simulate(&mut ctx_nat, &[ix_delegate_nat], &[&staker]).await;
    println!("delegate,{units_pin},{units_nat}");

    // Apply delegate so the stake account transitions to Stake state
    for (ctx, ix) in [(&mut ctx_pin, ixn::delegate_stake(&stake_a.pubkey(), &staker.pubkey(), &vote.pubkey())),
                      (&mut ctx_nat, sdk_stake_ixn::delegate_stake(&stake_a.pubkey(), &staker.pubkey(), &vote.pubkey()))] {
        let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
        let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
        tx.try_sign(&[&ctx.payer, &staker], ctx.last_blockhash).unwrap();
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }

    // 3) deactivate
    let ix_deact_pin = ixn::deactivate_stake(&stake_a.pubkey(), &staker.pubkey());
    let ix_deact_nat = sdk_stake_ixn::deactivate_stake(&stake_a.pubkey(), &staker.pubkey());
    let units_pin = simulate(&mut ctx_pin, &[ix_deact_pin], &[&staker]).await;
    let units_nat = simulate(&mut ctx_nat, &[ix_deact_nat], &[&staker]).await;
    println!("deactivate,{units_pin},{units_nat}");

    // Apply deactivate so withdraw/merge flows see deactivated stake when needed
    for (ctx, ix) in [(&mut ctx_pin, ixn::deactivate_stake(&stake_a.pubkey(), &staker.pubkey())),
                      (&mut ctx_nat, sdk_stake_ixn::deactivate_stake(&stake_a.pubkey(), &staker.pubkey()))] {
        let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
        let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
        tx.try_sign(&[&ctx.payer, &staker], ctx.last_blockhash).unwrap();
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }

    // 4) split (create destination and split a portion)
    let split_dest = solana_sdk::signature::Keypair::new();
    // Destination stake accounts must exist with proper size/owner
    create_stake_account_pin(&mut ctx_pin, &split_dest).await;
    create_stake_account_native(&mut ctx_nat, &split_dest).await;
    // Build split instructions via adapters (SDK returns system create + split)
    let split_lamports = 1_000_000_000u64; // 1 SOL
    let split_pin_all = ixn::split(&stake_a.pubkey(), &staker.pubkey(), split_lamports, &split_dest.pubkey());
    let split_nat_all = sdk_stake_ixn::split(&stake_a.pubkey(), &staker.pubkey(), split_lamports, &split_dest.pubkey());
    // Only keep the stake-program instruction; destination is already created
    let split_pin: Vec<_> = split_pin_all
        .into_iter()
        .filter(|ix| ix.program_id == solana_sdk::stake::program::id())
        .collect();
    let split_nat: Vec<_> = split_nat_all
        .into_iter()
        .filter(|ix| ix.program_id == solana_sdk::stake::program::id())
        .collect();
    let units_pin = simulate(&mut ctx_pin, &split_pin, &[&staker]).await;
    let units_nat = simulate(&mut ctx_nat, &split_nat, &[&staker]).await;
    println!("split,{units_pin},{units_nat}");

    // Apply split on both contexts
    for (ctx, v_all) in [(&mut ctx_pin, ixn::split(&stake_a.pubkey(), &staker.pubkey(), split_lamports, &split_dest.pubkey())),
                         (&mut ctx_nat, sdk_stake_ixn::split(&stake_a.pubkey(), &staker.pubkey(), split_lamports, &split_dest.pubkey()))] {
        let v: Vec<_> = v_all.into_iter().filter(|ix| ix.program_id == solana_sdk::stake::program::id()).collect();
        let msg = Message::new(&v, Some(&ctx.payer.pubkey()));
        let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
        tx.try_sign(&[&ctx.payer, &staker], ctx.last_blockhash).unwrap();
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }

    // 5) withdraw (small withdrawal from a prefunded initialized stake)
    let stake_w = solana_sdk::signature::Keypair::new();
    create_stake_account_pin(&mut ctx_pin, &stake_w).await;
    create_stake_account_native(&mut ctx_nat, &stake_w).await;
    let ix_w_init_pin = ixn::initialize_checked(&stake_w.pubkey(), &auth);
    let ix_w_init_nat = sdk_stake_ixn::initialize_checked(&stake_w.pubkey(), &auth);
    for (ctx, ix) in [(&mut ctx_pin, ix_w_init_pin), (&mut ctx_nat, ix_w_init_nat)] {
        let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
        let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
        tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }
    // Prefund above rent so withdraw is possible
    for ctx in [&mut ctx_pin, &mut ctx_nat] {
        let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
            &[system_instruction::transfer(&ctx.payer.pubkey(), &stake_w.pubkey(), 1_000_000_000)],
            Some(&ctx.payer.pubkey()),
            &[&ctx.payer],
            ctx.last_blockhash,
        );
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }
    let recipient = solana_sdk::signature::Keypair::new();
    let withdraw_lamports = 500_000_000u64; // 0.5 SOL
    let ix_w_pin = ixn::withdraw(&stake_w.pubkey(), &withdrawer.pubkey(), &recipient.pubkey(), withdraw_lamports, None);
    let ix_w_nat = sdk_stake_ixn::withdraw(&stake_w.pubkey(), &withdrawer.pubkey(), &recipient.pubkey(), withdraw_lamports, None);
    let units_pin = simulate(&mut ctx_pin, &[ix_w_pin.clone()], &[&withdrawer]).await;
    let units_nat = simulate(&mut ctx_nat, &[ix_w_nat.clone()], &[&withdrawer]).await;
    println!("withdraw,{units_pin},{units_nat}");
    for (ctx, ix) in [(&mut ctx_pin, ix_w_pin), (&mut ctx_nat, ix_w_nat)] {
        let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
        let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
        tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }

    // 6) merge (Initialized + Initialized)
    let stake_m1 = solana_sdk::signature::Keypair::new();
    let stake_m2 = solana_sdk::signature::Keypair::new();
    create_stake_account_pin(&mut ctx_pin, &stake_m1).await;
    create_stake_account_native(&mut ctx_nat, &stake_m1).await;
    create_stake_account_pin(&mut ctx_pin, &stake_m2).await;
    create_stake_account_native(&mut ctx_nat, &stake_m2).await;
    let auth_b = Authorized { staker: staker.pubkey(), withdrawer: new_withdrawer.pubkey() };
    let ix_m1_init_pin = ixn::initialize_checked(&stake_m1.pubkey(), &auth_b);
    let ix_m1_init_nat = sdk_stake_ixn::initialize_checked(&stake_m1.pubkey(), &auth_b);
    let ix_m2_init_pin = ixn::initialize_checked(&stake_m2.pubkey(), &auth_b);
    let ix_m2_init_nat = sdk_stake_ixn::initialize_checked(&stake_m2.pubkey(), &auth_b);
    for (ctx, ix) in [(&mut ctx_pin, ix_m1_init_pin), (&mut ctx_nat, ix_m1_init_nat)] {
        let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
        let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
        tx.try_sign(&[&ctx.payer, &new_withdrawer], ctx.last_blockhash).unwrap();
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }
    for (ctx, ix) in [(&mut ctx_pin, ix_m2_init_pin), (&mut ctx_nat, ix_m2_init_nat)] {
        let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
        let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
        tx.try_sign(&[&ctx.payer, &new_withdrawer], ctx.last_blockhash).unwrap();
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }
    // Build merge: dest = m1, src = m2 (both Initialized)
    let merge_pin = ixn::merge(&stake_m1.pubkey(), &stake_m2.pubkey(), &staker.pubkey());
    let merge_nat = sdk_stake_ixn::merge(&stake_m1.pubkey(), &stake_m2.pubkey(), &staker.pubkey());
    let units_pin = simulate(&mut ctx_pin, &merge_pin, &[&staker]).await;
    let units_nat = simulate(&mut ctx_nat, &merge_nat, &[&staker]).await;
    println!("merge,{units_pin},{units_nat}");
    // Apply merge
    for (ctx, v) in [(&mut ctx_pin, ixn::merge(&stake_m1.pubkey(), &stake_m2.pubkey(), &staker.pubkey())),
                     (&mut ctx_nat, sdk_stake_ixn::merge(&stake_m1.pubkey(), &stake_m2.pubkey(), &staker.pubkey()))] {
        let msg = Message::new(&v, Some(&ctx.payer.pubkey()));
        let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
        tx.try_sign(&[&ctx.payer, &staker], ctx.last_blockhash).unwrap();
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }

    // 7) move_lamports (between two Initialized stake accounts with matching authorities)
    let stake_c = solana_sdk::signature::Keypair::new();
    create_stake_account_pin(&mut ctx_pin, &stake_c).await;
    create_stake_account_native(&mut ctx_nat, &stake_c).await;
    // Initialize stake_c with the same authorities as stake_w (auth)
    let ix_c_init_pin = ixn::initialize_checked(&stake_c.pubkey(), &auth);
    let ix_c_init_nat = sdk_stake_ixn::initialize_checked(&stake_c.pubkey(), &auth);
    for (ctx, ix) in [(&mut ctx_pin, ix_c_init_pin), (&mut ctx_nat, ix_c_init_nat)] {
        let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
        let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
        tx.try_sign(&[&ctx.payer, &withdrawer], ctx.last_blockhash).unwrap();
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }
    // Move from stake_w (Initialized) to stake_c (Initialized)
    let ix_move_pin = ixn::move_lamports(&stake_w.pubkey(), &stake_c.pubkey(), &staker.pubkey(), 100_000_000);
    let ix_move_nat = sdk_stake_ixn::move_lamports(&stake_w.pubkey(), &stake_c.pubkey(), &staker.pubkey(), 100_000_000);
    let units_pin = simulate(&mut ctx_pin, &[ix_move_pin.clone()], &[&staker]).await;
    let units_nat = simulate(&mut ctx_nat, &[ix_move_nat.clone()], &[&staker]).await;
    println!("move_lamports,{units_pin},{units_nat}");
    // Apply move_lamports
    for (ctx, ix) in [(&mut ctx_pin, ix_move_pin), (&mut ctx_nat, ix_move_nat)] {
        let msg = Message::new(&[ix], Some(&ctx.payer.pubkey()));
        let mut tx = solana_sdk::transaction::Transaction::new_unsigned(msg);
        tx.try_sign(&[&ctx.payer, &staker], ctx.last_blockhash).unwrap();
        ctx.banks_client.process_transaction(tx).await.unwrap();
    }

    // 8) get_minimum_delegation (no signers)
    let ix_min_pin = ixn::get_minimum_delegation();
    let ix_min_nat = sdk_stake_ixn::get_minimum_delegation();
    let units_pin = simulate(&mut ctx_pin, &[ix_min_pin], &[]).await;
    let units_nat = simulate(&mut ctx_nat, &[ix_min_nat], &[]).await;
    println!("get_minimum_delegation,{units_pin},{units_nat}");
}
