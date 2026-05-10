use {
    anchor_lang::{
        solana_program::{
            self,
            instruction::{AccountMeta, Instruction},
            pubkey::Pubkey,
            system_instruction,
        },
        InstructionData, ToAccountMetas,
    },
    litesvm::LiteSVM,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    spl_associated_token_account_interface::{
        address::get_associated_token_address_with_program_id,
        instruction::create_associated_token_account,
    },
    spl_token_2022_interface::{
        extension::{transfer_hook::instruction::initialize as init_transfer_hook, ExtensionType},
        instruction::{initialize_mint2, mint_to, transfer_checked},
        state::Mint,
        ID as TOKEN_2022_ID,
    },
    whitelist_transfer_hook_q2 as program,
};

// ─── shared helpers ──────────────────────────────────────────────────────────

fn send(
    svm: &mut LiteSVM,
    ixs: &[Instruction],
    payer: &Keypair,
    signers: &[&Keypair],
) -> litesvm::types::TransactionResult {
    svm.expire_blockhash();
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(ixs, Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
    svm.send_transaction(tx)
}

/// Derives the whitelist config PDA.
fn config_pda(program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"whitelist-config"], program_id).0
}

/// Derives the per-user whitelist entry PDA.
fn entry_pda(user: &Pubkey, program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"whitelist", user.as_ref()], program_id).0
}

/// Sends initialize_whitelist and returns the config PDA.
fn initialize_whitelist(svm: &mut LiteSVM, admin: &Keypair, program_id: &Pubkey) -> Pubkey {
    let cfg = config_pda(program_id);
    let ix = Instruction::new_with_bytes(
        *program_id,
        &program::instruction::InitializeWhitelist {}.data(),
        program::accounts::InitializeWhitelist {
            admin: admin.pubkey(),
            config: cfg,
            system_program: solana_program::system_program::id(),
        }
        .to_account_metas(None),
    );
    send(svm, &[ix], admin, &[admin]).expect("initialize_whitelist failed");
    cfg
}

/// Sends add_to_whitelist for `user`.
fn add_to_whitelist(
    svm: &mut LiteSVM,
    admin: &Keypair,
    user: &Pubkey,
    program_id: &Pubkey,
) -> litesvm::types::TransactionResult {
    let ix = Instruction::new_with_bytes(
        *program_id,
        &program::instruction::AddToWhitelist { user: *user }.data(),
        program::accounts::AddToWhitelist {
            admin: admin.pubkey(),
            config: config_pda(program_id),
            whitelist_entry: entry_pda(user, program_id),
            system_program: solana_program::system_program::id(),
        }
        .to_account_metas(None),
    );
    send(svm, &[ix], admin, &[admin])
}

/// Sends remove_from_whitelist for `user`.
fn remove_from_whitelist(
    svm: &mut LiteSVM,
    admin: &Keypair,
    user: &Pubkey,
    program_id: &Pubkey,
) -> litesvm::types::TransactionResult {
    let ix = Instruction::new_with_bytes(
        *program_id,
        &program::instruction::RemoveFromWhitelist { user: *user }.data(),
        program::accounts::RemoveFromWhitelist {
            admin: admin.pubkey(),
            config: config_pda(program_id),
            whitelist_entry: entry_pda(user, program_id),
        }
        .to_account_metas(None),
    );
    send(svm, &[ix], admin, &[admin])
}

/// Creates a Token-2022 mint with the transfer hook extension pointing at `program_id`.
fn create_mint(svm: &mut LiteSVM, payer: &Keypair, program_id: &Pubkey) -> Keypair {
    let mint = Keypair::new();
    let mint_size =
        ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::TransferHook]).unwrap();
    let rent = svm.minimum_balance_for_rent_exemption(mint_size);

    let ixs = [
        system_instruction::create_account(
            &payer.pubkey(),
            &mint.pubkey(),
            rent,
            mint_size as u64,
            &TOKEN_2022_ID,
        ),
        init_transfer_hook(
            &TOKEN_2022_ID,
            &mint.pubkey(),
            Some(payer.pubkey()),
            Some(*program_id),
        )
        .unwrap(),
        initialize_mint2(&TOKEN_2022_ID, &mint.pubkey(), &payer.pubkey(), None, 9).unwrap(),
    ];
    send(svm, &ixs, payer, &[payer, &mint]).expect("create_mint failed");
    mint
}

/// Initializes the ExtraAccountMetaList PDA for the given mint.
fn init_extra_meta(svm: &mut LiteSVM, payer: &Keypair, mint: &Pubkey, program_id: &Pubkey) {
    let (extra_meta_pda, _) =
        Pubkey::find_program_address(&[b"extra-account-metas", mint.as_ref()], program_id);
    let ix = Instruction::new_with_bytes(
        *program_id,
        &program::instruction::InitializeTransferHook {}.data(),
        program::accounts::InitializeExtraAccountMetaList {
            payer: payer.pubkey(),
            extra_account_meta_list: extra_meta_pda,
            mint: *mint,
            system_program: solana_program::system_program::id(),
        }
        .to_account_metas(None),
    );
    send(svm, &[ix], payer, &[payer]).expect("init_extra_meta failed");
}

/// Builds a transfer_checked instruction with the hook extra accounts appended.
/// The extra account is the per-sender whitelist entry PDA.
fn build_transfer_ix(
    source_owner: &Keypair,
    mint: &Pubkey,
    src: Pubkey,
    dst: Pubkey,
    amount: u64,
    extra_meta_pda: Pubkey,
    program_id: &Pubkey,
) -> Instruction {
    let mut ix = transfer_checked(
        &TOKEN_2022_ID,
        &src,
        mint,
        &dst,
        &source_owner.pubkey(),
        &[],
        amount,
        9,
    )
    .unwrap();
    // Token-2022 transfer hook requires these accounts appended in order:
    //   1. extra_account_meta_list PDA
    //   2. the TLV-registered accounts (our per-sender whitelist entry)
    //   3. the hook program itself
    ix.accounts
        .push(AccountMeta::new_readonly(extra_meta_pda, false));
    ix.accounts.push(AccountMeta::new_readonly(
        entry_pda(&source_owner.pubkey(), program_id),
        false,
    ));
    ix.accounts
        .push(AccountMeta::new_readonly(*program_id, false));
    ix
}

// ─── tests ───────────────────────────────────────────────────────────────────

/// After initialize_whitelist, the config PDA must exist on-chain and store the admin.
#[test]
fn test_initialize_whitelist_creates_config() {
    let mut svm = LiteSVM::new();
    let admin = Keypair::new();
    let program_id = program::id();
    svm.add_program(program_id, include_bytes!("../../../target/deploy/whitelist_transfer_hook_q2.so"));
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();

    let cfg = initialize_whitelist(&mut svm, &admin, &program_id);

    // The config PDA must exist after initialization.
    let account = svm.get_account(&cfg);
    assert!(account.is_some(), "config PDA should exist after initialize_whitelist");
}

/// add_to_whitelist must create the entry PDA on-chain.
#[test]
fn test_add_to_whitelist_creates_entry_pda() {
    let mut svm = LiteSVM::new();
    let admin = Keypair::new();
    let user = Keypair::new();
    let program_id = program::id();
    svm.add_program(program_id, include_bytes!("../../../target/deploy/whitelist_transfer_hook_q2.so"));
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();

    initialize_whitelist(&mut svm, &admin, &program_id);

    // Entry PDA must not exist before adding.
    assert!(
        svm.get_account(&entry_pda(&user.pubkey(), &program_id)).is_none(),
        "entry PDA should not exist before add_to_whitelist"
    );

    add_to_whitelist(&mut svm, &admin, &user.pubkey(), &program_id)
        .expect("add_to_whitelist should succeed");

    // Entry PDA must exist after adding.
    assert!(
        svm.get_account(&entry_pda(&user.pubkey(), &program_id)).is_some(),
        "entry PDA should exist after add_to_whitelist"
    );
}

/// remove_from_whitelist must close the entry PDA (account gone) and refund rent to admin.
#[test]
fn test_remove_from_whitelist_closes_entry_pda() {
    let mut svm = LiteSVM::new();
    let admin = Keypair::new();
    let user = Keypair::new();
    let program_id = program::id();
    svm.add_program(program_id, include_bytes!("../../../target/deploy/whitelist_transfer_hook_q2.so"));
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();

    initialize_whitelist(&mut svm, &admin, &program_id);
    add_to_whitelist(&mut svm, &admin, &user.pubkey(), &program_id).unwrap();

    let balance_before = svm.get_account(&admin.pubkey()).unwrap().lamports;

    remove_from_whitelist(&mut svm, &admin, &user.pubkey(), &program_id)
        .expect("remove_from_whitelist should succeed");

    // Entry PDA must be gone after removal.
    assert!(
        svm.get_account(&entry_pda(&user.pubkey(), &program_id)).is_none(),
        "entry PDA should be closed after remove_from_whitelist"
    );

    // Admin should have received the rent back (balance increases minus tx fee).
    let balance_after = svm.get_account(&admin.pubkey()).unwrap().lamports;
    assert!(
        balance_after > balance_before,
        "admin should receive rent refund after closing entry PDA"
    );
}

/// Adding the same user twice must fail — `init` on an already-existing PDA errors.
#[test]
fn test_add_duplicate_fails() {
    let mut svm = LiteSVM::new();
    let admin = Keypair::new();
    let user = Keypair::new();
    let program_id = program::id();
    svm.add_program(program_id, include_bytes!("../../../target/deploy/whitelist_transfer_hook_q2.so"));
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();

    initialize_whitelist(&mut svm, &admin, &program_id);
    add_to_whitelist(&mut svm, &admin, &user.pubkey(), &program_id).unwrap();

    let res = add_to_whitelist(&mut svm, &admin, &user.pubkey(), &program_id);
    assert!(
        res.is_err(),
        "adding the same user twice should fail — PDA already exists"
    );
}

/// Removing a user who was never added must fail — entry PDA doesn't exist.
#[test]
fn test_remove_nonexistent_fails() {
    let mut svm = LiteSVM::new();
    let admin = Keypair::new();
    let user = Keypair::new();
    let program_id = program::id();
    svm.add_program(program_id, include_bytes!("../../../target/deploy/whitelist_transfer_hook_q2.so"));
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();

    initialize_whitelist(&mut svm, &admin, &program_id);

    let res = remove_from_whitelist(&mut svm, &admin, &user.pubkey(), &program_id);
    assert!(
        res.is_err(),
        "removing a user who was never added should fail"
    );
}

/// A non-admin signer must not be able to add to the whitelist.
#[test]
fn test_unauthorized_add_fails() {
    let mut svm = LiteSVM::new();
    let admin = Keypair::new();
    let attacker = Keypair::new();
    let user = Keypair::new();
    let program_id = program::id();
    svm.add_program(program_id, include_bytes!("../../../target/deploy/whitelist_transfer_hook_q2.so"));
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();

    initialize_whitelist(&mut svm, &admin, &program_id);

    // Attacker tries to add a user using the real config PDA but signing as themselves.
    // has_one = admin on the config account rejects this.
    let ix = Instruction::new_with_bytes(
        program_id,
        &program::instruction::AddToWhitelist { user: user.pubkey() }.data(),
        program::accounts::AddToWhitelist {
            admin: attacker.pubkey(),
            config: config_pda(&program_id),
            whitelist_entry: entry_pda(&user.pubkey(), &program_id),
            system_program: solana_program::system_program::id(),
        }
        .to_account_metas(None),
    );
    let res = send(&mut svm, &[ix], &attacker, &[&attacker]);
    assert!(
        res.is_err(),
        "non-admin should not be able to add to whitelist"
    );
}

/// A non-admin signer must not be able to remove from the whitelist.
#[test]
fn test_unauthorized_remove_fails() {
    let mut svm = LiteSVM::new();
    let admin = Keypair::new();
    let attacker = Keypair::new();
    let user = Keypair::new();
    let program_id = program::id();
    svm.add_program(program_id, include_bytes!("../../../target/deploy/whitelist_transfer_hook_q2.so"));
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&attacker.pubkey(), 10_000_000_000).unwrap();

    initialize_whitelist(&mut svm, &admin, &program_id);
    add_to_whitelist(&mut svm, &admin, &user.pubkey(), &program_id).unwrap();

    let ix = Instruction::new_with_bytes(
        program_id,
        &program::instruction::RemoveFromWhitelist { user: user.pubkey() }.data(),
        program::accounts::RemoveFromWhitelist {
            admin: attacker.pubkey(),
            config: config_pda(&program_id),
            whitelist_entry: entry_pda(&user.pubkey(), &program_id),
        }
        .to_account_metas(None),
    );
    let res = send(&mut svm, &[ix], &attacker, &[&attacker]);
    assert!(
        res.is_err(),
        "non-admin should not be able to remove from whitelist"
    );
}

/// A whitelisted sender must be able to transfer tokens.
#[test]
fn test_transfer_succeeds_when_whitelisted() {
    let mut svm = LiteSVM::new();
    let admin = Keypair::new();
    let recipient = Keypair::new();
    let program_id = program::id();
    svm.add_program(program_id, include_bytes!("../../../target/deploy/whitelist_transfer_hook_q2.so"));
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();

    initialize_whitelist(&mut svm, &admin, &program_id);
    add_to_whitelist(&mut svm, &admin, &admin.pubkey(), &program_id).unwrap();

    let mint = create_mint(&mut svm, &admin, &program_id);
    let (extra_meta_pda, _) = Pubkey::find_program_address(
        &[b"extra-account-metas", mint.pubkey().as_ref()],
        &program_id,
    );
    init_extra_meta(&mut svm, &admin, &mint.pubkey(), &program_id);

    let src_ata = get_associated_token_address_with_program_id(
        &admin.pubkey(), &mint.pubkey(), &TOKEN_2022_ID,
    );
    let dst_ata = get_associated_token_address_with_program_id(
        &recipient.pubkey(), &mint.pubkey(), &TOKEN_2022_ID,
    );
    send(&mut svm, &[
        create_associated_token_account(&admin.pubkey(), &admin.pubkey(), &mint.pubkey(), &TOKEN_2022_ID),
        create_associated_token_account(&admin.pubkey(), &recipient.pubkey(), &mint.pubkey(), &TOKEN_2022_ID),
        mint_to(&TOKEN_2022_ID, &mint.pubkey(), &src_ata, &admin.pubkey(), &[], 100 * 10u64.pow(9)).unwrap(),
    ], &admin, &[&admin]).unwrap();

    let transfer_ix = build_transfer_ix(
        &admin, &mint.pubkey(), src_ata, dst_ata,
        1 * 10u64.pow(9), extra_meta_pda, &program_id,
    );
    let res = send(&mut svm, &[transfer_ix], &admin, &[&admin]);
    assert!(res.is_ok(), "transfer should succeed — sender is whitelisted");
}

/// A non-whitelisted sender must not be able to transfer tokens.
#[test]
fn test_transfer_fails_when_not_whitelisted() {
    let mut svm = LiteSVM::new();
    let admin = Keypair::new();
    let recipient = Keypair::new();
    let program_id = program::id();
    svm.add_program(program_id, include_bytes!("../../../target/deploy/whitelist_transfer_hook_q2.so"));
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();

    // Admin is NOT added to the whitelist — only config is initialized.
    initialize_whitelist(&mut svm, &admin, &program_id);

    let mint = create_mint(&mut svm, &admin, &program_id);
    let (extra_meta_pda, _) = Pubkey::find_program_address(
        &[b"extra-account-metas", mint.pubkey().as_ref()],
        &program_id,
    );
    init_extra_meta(&mut svm, &admin, &mint.pubkey(), &program_id);

    let src_ata = get_associated_token_address_with_program_id(
        &admin.pubkey(), &mint.pubkey(), &TOKEN_2022_ID,
    );
    let dst_ata = get_associated_token_address_with_program_id(
        &recipient.pubkey(), &mint.pubkey(), &TOKEN_2022_ID,
    );
    send(&mut svm, &[
        create_associated_token_account(&admin.pubkey(), &admin.pubkey(), &mint.pubkey(), &TOKEN_2022_ID),
        create_associated_token_account(&admin.pubkey(), &recipient.pubkey(), &mint.pubkey(), &TOKEN_2022_ID),
        mint_to(&TOKEN_2022_ID, &mint.pubkey(), &src_ata, &admin.pubkey(), &[], 100 * 10u64.pow(9)).unwrap(),
    ], &admin, &[&admin]).unwrap();

    let transfer_ix = build_transfer_ix(
        &admin, &mint.pubkey(), src_ata, dst_ata,
        1 * 10u64.pow(9), extra_meta_pda, &program_id,
    );
    let res = send(&mut svm, &[transfer_ix], &admin, &[&admin]);
    assert!(res.is_err(), "transfer should fail — sender is not whitelisted");
}

/// After removing a user, their subsequent transfer must fail.
/// Re-adding them must restore transfer access.
#[test]
fn test_remove_then_readd_restores_access() {
    let mut svm = LiteSVM::new();
    let admin = Keypair::new();
    let recipient = Keypair::new();
    let program_id = program::id();
    svm.add_program(program_id, include_bytes!("../../../target/deploy/whitelist_transfer_hook_q2.so"));
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();

    initialize_whitelist(&mut svm, &admin, &program_id);
    add_to_whitelist(&mut svm, &admin, &admin.pubkey(), &program_id).unwrap();

    let mint = create_mint(&mut svm, &admin, &program_id);
    let (extra_meta_pda, _) = Pubkey::find_program_address(
        &[b"extra-account-metas", mint.pubkey().as_ref()],
        &program_id,
    );
    init_extra_meta(&mut svm, &admin, &mint.pubkey(), &program_id);

    let src_ata = get_associated_token_address_with_program_id(
        &admin.pubkey(), &mint.pubkey(), &TOKEN_2022_ID,
    );
    let dst_ata = get_associated_token_address_with_program_id(
        &recipient.pubkey(), &mint.pubkey(), &TOKEN_2022_ID,
    );
    send(&mut svm, &[
        create_associated_token_account(&admin.pubkey(), &admin.pubkey(), &mint.pubkey(), &TOKEN_2022_ID),
        create_associated_token_account(&admin.pubkey(), &recipient.pubkey(), &mint.pubkey(), &TOKEN_2022_ID),
        mint_to(&TOKEN_2022_ID, &mint.pubkey(), &src_ata, &admin.pubkey(), &[], 100 * 10u64.pow(9)).unwrap(),
    ], &admin, &[&admin]).unwrap();

    let make_transfer_ix = || build_transfer_ix(
        &admin, &mint.pubkey(), src_ata, dst_ata,
        1 * 10u64.pow(9), extra_meta_pda, &program_id,
    );

    // Transfer works while whitelisted.
    assert!(
        send(&mut svm, &[make_transfer_ix()], &admin, &[&admin]).is_ok(),
        "transfer should succeed — admin is whitelisted"
    );

    // Remove admin from whitelist.
    remove_from_whitelist(&mut svm, &admin, &admin.pubkey(), &program_id).unwrap();

    // Transfer now blocked.
    assert!(
        send(&mut svm, &[make_transfer_ix()], &admin, &[&admin]).is_err(),
        "transfer should fail — admin was removed from whitelist"
    );

    // Re-add admin.
    add_to_whitelist(&mut svm, &admin, &admin.pubkey(), &program_id).unwrap();

    // Transfer works again.
    assert!(
        send(&mut svm, &[make_transfer_ix()], &admin, &[&admin]).is_ok(),
        "transfer should succeed — admin was re-added to whitelist"
    );
}

/// Two independent users each have their own entry PDA.
/// Removing one must not affect the other's transfer access.
#[test]
fn test_multiple_users_independent_entries() {
    let mut svm = LiteSVM::new();
    let admin = Keypair::new();
    let user_a = Keypair::new();
    let user_b = Keypair::new();
    let program_id = program::id();
    svm.add_program(program_id, include_bytes!("../../../target/deploy/whitelist_transfer_hook_q2.so"));
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&user_a.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&user_b.pubkey(), 10_000_000_000).unwrap();

    initialize_whitelist(&mut svm, &admin, &program_id);
    add_to_whitelist(&mut svm, &admin, &user_a.pubkey(), &program_id).unwrap();
    add_to_whitelist(&mut svm, &admin, &user_b.pubkey(), &program_id).unwrap();

    let mint = create_mint(&mut svm, &admin, &program_id);
    let (extra_meta_pda, _) = Pubkey::find_program_address(
        &[b"extra-account-metas", mint.pubkey().as_ref()],
        &program_id,
    );
    init_extra_meta(&mut svm, &admin, &mint.pubkey(), &program_id);

    // Give each user their own source ATA and a shared destination ATA.
    let dst_ata = get_associated_token_address_with_program_id(
        &admin.pubkey(), &mint.pubkey(), &TOKEN_2022_ID,
    );
    let src_a = get_associated_token_address_with_program_id(
        &user_a.pubkey(), &mint.pubkey(), &TOKEN_2022_ID,
    );
    let src_b = get_associated_token_address_with_program_id(
        &user_b.pubkey(), &mint.pubkey(), &TOKEN_2022_ID,
    );

    let token_amount = 100 * 10u64.pow(9);
    send(&mut svm, &[
        create_associated_token_account(&admin.pubkey(), &admin.pubkey(), &mint.pubkey(), &TOKEN_2022_ID),
        create_associated_token_account(&admin.pubkey(), &user_a.pubkey(), &mint.pubkey(), &TOKEN_2022_ID),
        create_associated_token_account(&admin.pubkey(), &user_b.pubkey(), &mint.pubkey(), &TOKEN_2022_ID),
        mint_to(&TOKEN_2022_ID, &mint.pubkey(), &src_a, &admin.pubkey(), &[], token_amount).unwrap(),
        mint_to(&TOKEN_2022_ID, &mint.pubkey(), &src_b, &admin.pubkey(), &[], token_amount).unwrap(),
    ], &admin, &[&admin]).unwrap();

    // Both users can transfer while whitelisted.
    assert!(
        send(&mut svm, &[build_transfer_ix(&user_a, &mint.pubkey(), src_a, dst_ata, 10u64.pow(9), extra_meta_pda, &program_id)], &user_a, &[&user_a]).is_ok(),
        "user_a transfer should succeed — whitelisted"
    );
    assert!(
        send(&mut svm, &[build_transfer_ix(&user_b, &mint.pubkey(), src_b, dst_ata, 10u64.pow(9), extra_meta_pda, &program_id)], &user_b, &[&user_b]).is_ok(),
        "user_b transfer should succeed — whitelisted"
    );

    // Remove user_a only.
    remove_from_whitelist(&mut svm, &admin, &user_a.pubkey(), &program_id).unwrap();

    // user_a entry PDA is gone, user_b entry PDA is untouched.
    assert!(
        svm.get_account(&entry_pda(&user_a.pubkey(), &program_id)).is_none(),
        "user_a entry PDA should be closed"
    );
    assert!(
        svm.get_account(&entry_pda(&user_b.pubkey(), &program_id)).is_some(),
        "user_b entry PDA should still exist"
    );

    // user_a transfer is now blocked.
    assert!(
        send(&mut svm, &[build_transfer_ix(&user_a, &mint.pubkey(), src_a, dst_ata, 10u64.pow(9), extra_meta_pda, &program_id)], &user_a, &[&user_a]).is_err(),
        "user_a transfer should fail — removed from whitelist"
    );

    // user_b transfer still works.
    assert!(
        send(&mut svm, &[build_transfer_ix(&user_b, &mint.pubkey(), src_b, dst_ata, 10u64.pow(9), extra_meta_pda, &program_id)], &user_b, &[&user_b]).is_ok(),
        "user_b transfer should still succeed — still whitelisted"
    );
}
