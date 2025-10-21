use std::rc::Rc;
use std::sync::Arc;
use std::{time::Duration, time::Instant};

use barretenberg::Prove;
use burn_substitutor::BurnSubstitutor;
use contracts::{Address, Client, ConfirmationType};
use element::Element;
use ethereum_types::{H160, U256};
use hash::hash_merge;
use hex::ToHex;
use primitives::{block_height::BlockHeight, hash::CryptoHash, pagination::CursorChoice};
use testutil::eth::{EthNode, EthNodeOptions};
use zk_primitives::{InputNote, Note, Utxo, bridged_polygon_usdc_note_kind};

use crate::rpc::{
    ElementResponse, ListBlocksOrder, ListBlocksQuery, ListTxnOrder, ListTxnsQuery, ServerConfig,
    burn, mint, mint_with_note, rollup_contract, usdc_contract,
};

use super::Server;

fn extract_error_code(err: &serde_json::Value) -> String {
    let error_obj = err.get("error").expect("expected error object in response");

    let primary_code = error_obj
        .get("code")
        .and_then(|code| code.as_str())
        .unwrap_or_default();

    if primary_code != "bad-request" {
        return primary_code.to_owned();
    }

    error_obj
        .get("details")
        .and_then(|details| details.get("code"))
        .and_then(|code| code.as_str())
        .map(|code| code.to_owned())
        .or_else(|| {
            error_obj
                .get("reason")
                .and_then(|reason| reason.as_str())
                .map(|reason| reason.to_owned())
        })
        .or_else(|| {
            error_obj
                .get("data")
                .and_then(|data| data.get("code"))
                .and_then(|code| code.as_str())
                .map(|code| code.to_owned())
        })
        .unwrap_or_else(|| primary_code.to_owned())
}

macro_rules! expect_root_hash {
    ($server:expr, $root_hash:expr) => {
        if option_env!("TEMP_NOIR") == Some("1") {
        } else {
            let resp = $server.height().await.unwrap();
            $root_hash.assert_debug_eq(&resp.root_hash);
        }
    };
}

const ALLOWED_DUPLICATE_CODES: &[&str] = &[
    "commitment-already-pending",
    "duplicate-output-commitments",
    "duplicate-input-commitments",
    "already-exists",
];

const ALLOWED_DUPLICATE_INPUT_CODES: &[&str] = &[
    "commitment-already-pending",
    "duplicate-input-commitments",
    "input-commitments-not-found",
    "already-exists",
];

#[tokio::test(flavor = "multi_thread")]
async fn mint_transaction_not_in_contract() {
    let eth_node = EthNode::default().run_and_deploy().await;
    let mut server_config = ServerConfig::single_node(false);
    server_config.safe_eth_height_offset = 1;
    let server = Server::setup_and_wait(server_config, Arc::clone(&eth_node)).await;
    let rollup = rollup_contract(server.rollup_contract_addr, &eth_node).await;
    let usdc = usdc_contract(&rollup, &eth_node).await;

    let root_hash_before = server.height().await.unwrap().root_hash;
    let alice_pk = Element::new(0xA11CE);

    let (_note, _eth_tx, node_tx) = mint(
        &rollup,
        &usdc,
        &server,
        alice_pk,
        Element::from(100u64),
        Element::ZERO,
    );
    let time_before_sending_node_txn = Instant::now();
    tokio::spawn({
        let usdc = usdc.clone();
        let client = Client::from_eth_node(&eth_node);

        async move {
            // wait for node to receive the txn
            tokio::time::sleep(Duration::from_secs(2)).await;
            // progress the eth chain by 2 blocks by sending transactions
            client
                .wait_for_confirm(
                    usdc.approve(H160::from_low_u64_be(1), 1).await.unwrap(),
                    Duration::from_secs(1),
                    ConfirmationType::Latest,
                )
                .await
                .unwrap();
            client
                .wait_for_confirm(
                    usdc.approve(H160::from_low_u64_be(1), 1).await.unwrap(),
                    Duration::from_secs(1),
                    ConfirmationType::Latest,
                )
                .await
                .unwrap();
        }
    });
    let Err(err) = node_tx.await else {
        panic!("Expected mint to fail, got Ok");
    };
    assert!(
        time_before_sending_node_txn.elapsed() > Duration::from_secs(2),
        "time_before_sending_node_txn.elapsed() was expected to be more than 2 seconds, but is: {:?}",
        time_before_sending_node_txn.elapsed()
    );

    assert_eq!(
        err.get("error").unwrap().get("reason").unwrap(),
        &serde_json::Value::String("mint-not-in-contract".to_owned())
    );

    let resp = server.height().await.unwrap();
    // Root hash should not change
    assert_eq!(root_hash_before, resp.root_hash);

    if option_env!("TEMP_NOIR") == Some("1") {
    } else {
        expect_root_hash!(
            server,
            expect_test::expect![[r#"
            0x577b5b4aa3eaba75b2a919d5d7c63b7258aa507d38e346bf2ff1d48790379ff
        "#]]
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn mint_transaction_only() {
    let eth_node = EthNode::default().run_and_deploy().await;
    let mut server_config = ServerConfig::single_node(false);
    server_config.safe_eth_height_offset = 1;
    let server = Server::setup_and_wait(server_config, Arc::clone(&eth_node)).await;
    let rollup = rollup_contract(server.rollup_contract_addr, &eth_node).await;
    let usdc = usdc_contract(&rollup, &eth_node).await;

    let root_hash_before = server.height().await.unwrap().root_hash;
    let alice_pk = Element::new(0xA11CE);
    let alice_address = hash_merge([alice_pk, Element::ZERO]);
    let (alice_note, eth_tx, node_tx) = mint(
        &rollup,
        &usdc,
        &server,
        alice_address,
        Element::from(100u64),
        Element::ZERO,
    );
    eth_tx.await.unwrap();
    let time_before_sending_node_txn = Instant::now();
    tokio::spawn(async move {
        // wait for node to receive the txn
        tokio::time::sleep(Duration::from_secs(2)).await;
        // progress the eth chain by 1 block by sending a transaction
        usdc.approve(H160::from_low_u64_be(2), 1).await.unwrap();
    });
    let tx = node_tx.await.unwrap();
    assert!(
        time_before_sending_node_txn.elapsed() > Duration::from_secs(2),
        "time_before_sending_node_txn.elapsed() was expected to be more than 2 seconds, but is: {:?}",
        time_before_sending_node_txn.elapsed()
    );

    let resp = server.height().await.unwrap();
    assert_ne!(root_hash_before, resp.root_hash);
    assert_eq!(tx.root_hash, resp.root_hash);

    let element_info = server.element(alice_note.commitment()).await.unwrap();
    assert_eq!(
        element_info,
        ElementResponse {
            element: alice_note.commitment(),
            height: tx.height.0,
            root_hash: tx.root_hash,
            txn_hash: tx.txn_hash,
        }
    );

    if option_env!("TEMP_NOIR") == Some("1") {
    } else {
        expect_root_hash!(
            server,
            expect_test::expect![[r#"
                0xea000ebbc4e827874e8f3743b6c68765ea7d513731625f595139bf56381827
            "#]]
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn mint_and_transfer_alice_to_bob() {
    let eth_node = EthNode::default().run_and_deploy().await;
    let server =
        Server::setup_and_wait(ServerConfig::single_node(false), Arc::clone(&eth_node)).await;
    let rollup = rollup_contract(server.rollup_contract_addr, &eth_node).await;
    let usdc = usdc_contract(&rollup, &eth_node).await;

    let alice_pk = Element::new(0xA11CE);
    let alice_address = hash_merge([alice_pk, Element::ZERO]);
    let (alice_note, eth_tx, tx) = mint(
        &rollup,
        &usdc,
        &server,
        alice_address,
        Element::from(100u64),
        Element::ZERO,
    );
    eth_tx.await.unwrap();
    let tx = tx.await.unwrap();

    let bob_pk = Element::new(0xB0B);
    let bob_address = hash_merge([bob_pk, Element::ZERO]);
    let bob_note = Note {
        kind: Element::new(2),
        contract: bridged_polygon_usdc_note_kind(),
        address: bob_address,
        psi: Element::new(0),
        value: Element::new(100),
    };

    let input_note = InputNote::new(alice_note.clone(), alice_pk);
    let utxo = Utxo::new_send(
        [input_note.clone(), InputNote::padding_note()],
        [bob_note, Note::padding_note()],
    );

    // let snark = cache_utxo_proof("mint_and_transfer_alice_to_bob", &utxo);
    let snark = utxo.prove().unwrap();
    let resp = server.transaction(&snark).await.unwrap();
    assert_ne!(tx.root_hash, resp.root_hash);

    expect_root_hash!(
        server,
        expect_test::expect![[r#"
            0xf478b616bd7df6d0443336bf26784eeefb226444106ddb5135c58cdc6927e99
        "#]]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn double_spend() {
    let eth_node = EthNode::default().run_and_deploy().await;
    let server =
        Server::setup_and_wait(ServerConfig::single_node(false), Arc::clone(&eth_node)).await;
    let rollup = rollup_contract(server.rollup_contract_addr, &eth_node).await;
    let usdc = usdc_contract(&rollup, &eth_node).await;

    let alice_pk = Element::new(0xA11CE);
    let alice_address = hash_merge([alice_pk, Element::ZERO]);
    let (alice_note, eth_tx, tx) = mint(
        &rollup,
        &usdc,
        &server,
        alice_address,
        Element::from(100u64),
        Element::ZERO,
    );
    eth_tx.await.unwrap();
    let _tx = tx.await.unwrap();

    let bob_pk = Element::new(0xB0B);
    let bob_address = hash_merge([bob_pk, Element::ZERO]);
    let bob_note = Note::new_with_psi(bob_address, Element::from(100u64), Element::ZERO);

    let input_note = InputNote::new(alice_note.clone(), alice_pk);
    let utxo = Utxo::new_send(
        [input_note.clone(), InputNote::padding_note()],
        [bob_note, Note::padding_note()],
    );

    // let snark = cache_utxo_proof("double_spend", &utxo);
    let snark = utxo.prove().unwrap();

    let resp_1 = server.transaction(&snark);

    let bob_note_2 = Note::new_with_psi(bob_address, Element::from(100u64), Element::new(1));
    let utxo = Utxo::new_send(
        [input_note.clone(), InputNote::padding_note()],
        [bob_note_2, Note::padding_note()],
    );

    // let snark_2 = cache_utxo_proof("double_spend-2", &utxo);
    let snark_2 = utxo.prove().unwrap();

    let resp_2 = server.transaction(&snark_2);

    let (resp_1, resp_2) = tokio::join!(resp_1, resp_2);

    match (resp_1, resp_2) {
        (Ok(_), Err(err)) => {
            let code = extract_error_code(&err);
            assert!(
                ALLOWED_DUPLICATE_INPUT_CODES.contains(&code.as_str()),
                "unexpected error code {code}"
            );

            expect_root_hash!(
                server,
                expect_test::expect![[r#"
                    0xf478b616bd7df6d0443336bf26784eeefb226444106ddb5135c58cdc6927e99
                "#]]
            );
        }
        (Err(err), Ok(_)) => {
            let code = extract_error_code(&err);
            assert!(
                ALLOWED_DUPLICATE_INPUT_CODES.contains(&code.as_str()),
                "unexpected error code {code}"
            );

            expect_root_hash!(
                server,
                expect_test::expect![[r#"
                    0x27a9c15038cc9786757ea83814c73e70781f481d2504ca845cc1a5ba42ab8f11
                "#]]
            );
        }
        (Ok(_), Ok(_)) => {
            panic!("Expected one of the transactions to fail, got Ok on both");
        }
        (Err(_), Err(_)) => {
            panic!("Expected one of the transactions to succeed, got Err on both");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn send_transaction_with_duplicate_inputs_is_rejected() {
    let eth_node = EthNode::default().run_and_deploy().await;
    let server =
        Server::setup_and_wait(ServerConfig::single_node(false), Arc::clone(&eth_node)).await;
    let rollup = rollup_contract(server.rollup_contract_addr, &eth_node).await;
    let usdc = usdc_contract(&rollup, &eth_node).await;

    let alice_pk = Element::new(0xA11CE);
    let alice_address = hash_merge([alice_pk, Element::ZERO]);
    let (alice_note, eth_tx, rpc_tx) = mint(
        &rollup,
        &usdc,
        &server,
        alice_address,
        Element::from(100u64),
        Element::ZERO,
    );
    eth_tx.await.unwrap();
    rpc_tx.await.unwrap();

    let bob_pk = Element::new(0xB0B);
    let bob_address = hash_merge([bob_pk, Element::ZERO]);
    let duplicate_output = Note::new_with_psi(bob_address, Element::from(200u64), Element::ZERO);

    let input_note = InputNote::new(alice_note.clone(), alice_pk);
    let utxo = Utxo::new_send(
        [input_note.clone(), input_note.clone()],
        [duplicate_output, Note::padding_note()],
    );
    let proof = utxo.prove().unwrap();

    let res = server.transaction(&proof).await;
    match res {
        Ok(_) => panic!("duplicate inputs should be rejected"),
        Err(err) => {
            assert_eq!(extract_error_code(&err), "duplicate-input-commitments");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn two_transactions_with_duplicate_output_should_conflict() {
    let eth_node = EthNode::default().run_and_deploy().await;
    let server =
        Server::setup_and_wait(ServerConfig::single_node(false), Arc::clone(&eth_node)).await;
    let rollup = rollup_contract(server.rollup_contract_addr, &eth_node).await;
    let usdc = usdc_contract(&rollup, &eth_node).await;

    let alice_pk = Element::new(0xA11CE);
    let alice_address = hash_merge([alice_pk, Element::ZERO]);
    let (alice_note, alice_eth_tx, alice_rpc_tx) = mint(
        &rollup,
        &usdc,
        &server,
        alice_address,
        Element::from(100u64),
        Element::ZERO,
    );

    let charlie_pk = Element::new(0xC0FFEE);
    let charlie_address = hash_merge([charlie_pk, Element::ZERO]);
    let (charlie_note, charlie_eth_tx, charlie_rpc_tx) = mint(
        &rollup,
        &usdc,
        &server,
        charlie_address,
        Element::from(100u64),
        Element::from(1u64),
    );

    alice_eth_tx.await.unwrap();
    alice_rpc_tx.await.unwrap();
    charlie_eth_tx.await.unwrap();
    charlie_rpc_tx.await.unwrap();

    let bob_pk = Element::new(0xB0B);
    let bob_address = hash_merge([bob_pk, Element::ZERO]);
    let duplicated_output = Note::new_with_psi(bob_address, Element::from(100u64), Element::ZERO);

    let alice_input = InputNote::new(alice_note.clone(), alice_pk);
    let charlie_input = InputNote::new(charlie_note.clone(), charlie_pk);

    let utxo_1 = Utxo::new_send(
        [alice_input.clone(), InputNote::padding_note()],
        [duplicated_output.clone(), Note::padding_note()],
    );
    let utxo_2 = Utxo::new_send(
        [charlie_input.clone(), InputNote::padding_note()],
        [duplicated_output.clone(), Note::padding_note()],
    );

    let proof_1 = utxo_1.prove().unwrap();
    let proof_2 = utxo_2.prove().unwrap();

    let (res_1, res_2) = tokio::join!(server.transaction(&proof_1), server.transaction(&proof_2),);

    match (res_1, res_2) {
        (Ok(_), Err(err)) | (Err(err), Ok(_)) => {
            let code = extract_error_code(&err);
            assert!(
                ALLOWED_DUPLICATE_CODES.contains(&code.as_str()),
                "unexpected error code {code}"
            );
        }
        (Err(err1), Err(err2)) => {
            let code1 = extract_error_code(&err1);
            let code2 = extract_error_code(&err2);

            assert!(ALLOWED_DUPLICATE_CODES.contains(&code1.as_str()));
            assert!(ALLOWED_DUPLICATE_CODES.contains(&code2.as_str()));
        }
        (Ok(_), Ok(_)) => panic!("both transactions with duplicate outputs succeeded unexpectedly"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn two_transactions_with_duplicate_input_should_conflict() {
    let eth_node = EthNode::default().run_and_deploy().await;
    let server =
        Server::setup_and_wait(ServerConfig::single_node(false), Arc::clone(&eth_node)).await;
    let rollup = rollup_contract(server.rollup_contract_addr, &eth_node).await;
    let usdc = usdc_contract(&rollup, &eth_node).await;

    let alice_pk = Element::new(0xA11CE);
    let alice_address = hash_merge([alice_pk, Element::ZERO]);
    let (alice_note, alice_eth_tx, alice_rpc_tx) = mint(
        &rollup,
        &usdc,
        &server,
        alice_address,
        Element::from(100u64),
        Element::ZERO,
    );

    alice_eth_tx.await.unwrap();
    alice_rpc_tx.await.unwrap();

    let bob_pk = Element::new(0xB0B);
    let bob_address = hash_merge([bob_pk, Element::ZERO]);
    let bob_note_1 = Note::new_with_psi(bob_address, Element::from(100u64), Element::ZERO);
    let bob_note_2 = Note::new_with_psi(bob_address, Element::from(100u64), Element::from(1u64));

    let alice_input = InputNote::new(alice_note.clone(), alice_pk);

    let utxo_1 = Utxo::new_send(
        [alice_input.clone(), InputNote::padding_note()],
        [bob_note_1, Note::padding_note()],
    );
    let utxo_2 = Utxo::new_send(
        [alice_input.clone(), InputNote::padding_note()],
        [bob_note_2, Note::padding_note()],
    );

    let proof_1 = utxo_1.prove().unwrap();
    let proof_2 = utxo_2.prove().unwrap();

    let (res_1, res_2) = tokio::join!(server.transaction(&proof_1), server.transaction(&proof_2),);

    match (res_1, res_2) {
        (Ok(_), Err(err)) | (Err(err), Ok(_)) => {
            let code = extract_error_code(&err);
            assert!(
                ALLOWED_DUPLICATE_INPUT_CODES.contains(&code.as_str()),
                "unexpected error code {code}"
            );
        }
        (Err(err1), Err(err2)) => {
            let code1 = extract_error_code(&err1);
            let code2 = extract_error_code(&err2);

            assert!(ALLOWED_DUPLICATE_INPUT_CODES.contains(&code1.as_str()));
            assert!(ALLOWED_DUPLICATE_INPUT_CODES.contains(&code2.as_str()));
        }
        (Ok(_), Ok(_)) => panic!("both transactions with duplicate inputs succeeded unexpectedly"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn burn_tx() {
    let eth_node = EthNode::new(EthNodeOptions {
        use_noop_verifier: true,
        ..Default::default()
    })
    .run_and_deploy()
    .await;

    let server =
        Server::setup_and_wait(ServerConfig::single_node(false), Arc::clone(&eth_node)).await;
    let mut prover_server = Server::new(ServerConfig::mock_prover(false), Arc::clone(&eth_node));
    prover_server.set_peers(&[server.to_peer()]);
    prover_server.run(None);
    prover_server.wait().await.unwrap();

    let rollup = rollup_contract(server.rollup_contract_addr, &eth_node).await;
    let usdc = usdc_contract(&rollup, &eth_node).await;

    let alice_pk = Element::new(0xA11CE);
    let alice_address = hash_merge([alice_pk, Element::ZERO]);
    let (alice_note, eth_tx, tx) = mint(
        &rollup,
        &usdc,
        &server,
        alice_address,
        Element::from(100u64),
        Element::ZERO,
    );
    eth_tx.await.unwrap();
    let _tx = tx.await.unwrap();

    let input_note = InputNote::new(alice_note.clone(), alice_pk);

    let to = Address::from_low_u64_be(1);
    let (eth_tx, tx) = burn(&server, &input_note, &to);
    eth_tx.await.unwrap();

    let tx_resp = tx.await.unwrap();

    for i in 0.. {
        let height = rollup.block_height().await.unwrap();
        if height == tx_resp.height.0 {
            break;
        }

        if i == 120 {
            panic!("Failed to wait for tx to be included in a block");
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    let balance = usdc.balance(to).await.unwrap();
    assert_eq!(balance, U256::from(100));

    expect_root_hash!(
        server,
        expect_test::expect![[r#"
            0x577b5b4aa3eaba75b2a919d5d7c63b7258aa507d38e346bf2ff1d48790379ff
        "#]]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn substitute_burn_to_address() {
    let eth_node = EthNode::new(EthNodeOptions {
        use_noop_verifier: true,
        ..Default::default()
    })
    .run_and_deploy()
    .await;

    let server =
        Server::setup_and_wait(ServerConfig::single_node(false), Arc::clone(&eth_node)).await;
    let mut prover_server = Server::new(ServerConfig::mock_prover(false), Arc::clone(&eth_node));
    prover_server.set_peers(&[server.to_peer()]);
    prover_server.run(None);
    prover_server.wait().await.unwrap();

    let rollup = rollup_contract(server.rollup_contract_addr, &eth_node).await;
    let usdc = usdc_contract(&rollup, &eth_node).await;

    let mut burn_substitutor = BurnSubstitutor::new(
        rollup.clone(),
        usdc.clone(),
        server
            .base_url()
            .to_string()
            .trim_end_matches('/')
            .to_owned(),
        Duration::from_millis(50),
    );

    let alice_pk = Element::new(0xA11CE);
    let alice_address = hash_merge([alice_pk, Element::ZERO]);
    let (alice_note, eth_tx, tx) = mint(
        &rollup,
        &usdc,
        &server,
        alice_address,
        Element::from(100u64),
        Element::ZERO,
    );
    eth_tx.await.unwrap();
    let _tx = tx.await.unwrap();

    let rollup_balance = usdc.balance(rollup.address()).await.unwrap();
    assert_eq!(rollup_balance, U256::from(100));

    let input_note = InputNote::new(alice_note.clone(), alice_pk);

    let to = Address::from_low_u64_be(1);

    let (eth_tx, tx) = burn(&server, &input_note, &to);
    eth_tx.await.unwrap();

    let tx_resp = tx.await.unwrap();

    let substitutor_balance_before = usdc.balance(rollup.signer_address).await.unwrap();

    let substituted_burns = burn_substitutor.tick().await.unwrap();
    assert_eq!(substituted_burns.len(), 1);

    let balance = usdc.balance(to).await.unwrap();
    assert_eq!(balance, U256::from(100));

    assert_eq!(
        usdc.balance(rollup.signer_address).await.unwrap(),
        substitutor_balance_before - U256::from(100),
    );

    let rollup_balance = usdc.balance(rollup.address()).await.unwrap();
    assert_eq!(rollup_balance, U256::from(100));

    for i in 0.. {
        let height = rollup.block_height().await.unwrap();
        if height == tx_resp.height.0 {
            break;
        }

        if i == 10 {
            panic!("Failed to wait for tx to be included in a block");
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    let balance = usdc.balance(to).await.unwrap();
    assert_eq!(balance, U256::from(100));

    let rollup_balance = usdc.balance(rollup.address()).await.unwrap();
    assert_eq!(rollup_balance, U256::from(0));

    assert_eq!(
        usdc.balance(rollup.signer_address).await.unwrap(),
        substitutor_balance_before,
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn double_mint() {
    let eth_node = EthNode::default().run_and_deploy().await;
    let server =
        Server::setup_and_wait(ServerConfig::single_node(false), Arc::clone(&eth_node)).await;
    let rollup = rollup_contract(server.rollup_contract_addr, &eth_node).await;
    let usdc = usdc_contract(&rollup, &eth_node).await;

    let alice_pk = Element::new(0xA11CE);
    let alice_address = hash_merge([alice_pk, Element::ZERO]);
    let (alice_note, eth_tx, tx) = mint(
        &rollup,
        &usdc,
        &server,
        alice_address,
        Element::from(100u64),
        Element::ZERO,
    );
    eth_tx.await.unwrap();
    let _tx = tx.await.unwrap();

    let (_eth_tx, tx) = mint_with_note(&rollup, &usdc, &server, alice_note.clone());

    let err = tx.await.unwrap_err();
    assert_eq!(
        err.get("error").unwrap().get("reason").unwrap(),
        &serde_json::Value::String("output-commitments-exists".to_owned())
    );

    expect_root_hash!(
        server,
        expect_test::expect![[r#"
            0xea000ebbc4e827874e8f3743b6c68765ea7d513731625f595139bf56381827
        "#]]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn double_mint_same_mint_hash_different_address() {
    let eth_node = EthNode::default().run_and_deploy().await;

    let server =
        super::Server::setup_and_wait(ServerConfig::single_node(false), Arc::clone(&eth_node))
            .await;

    let rollup = rollup_contract(server.rollup_contract_addr, &eth_node).await;

    let alice_pk = Element::new(0xA11CE);
    let alice_address = hash::hash_merge([alice_pk, Element::ZERO]);

    let bob_pk = Element::new(0xB0B);
    let bob_address = hash::hash_merge([bob_pk, Element::ZERO]);

    let psi = Element::ZERO;
    let value = Element::from(100u64);
    let note_kind = bridged_polygon_usdc_note_kind();

    // Compute mint_hash (same for both since same psi)
    let mint_hash = hash::hash_merge([psi, Element::ZERO]);

    // Submit to EVM
    rollup.mint(&mint_hash, &value, &note_kind).await.unwrap();

    // Mint note A for Alice
    let alice_note = Note::new_with_psi(alice_address, value, psi);
    let utxo_a = Utxo::new_mint([alice_note.clone(), Note::padding_note()]);
    let proof_a = utxo_a.prove().unwrap();
    let _tx_a = server.transaction(&proof_a).await.unwrap();

    // Mint note B for Bob with same psi
    let bob_note = Note::new_with_psi(bob_address, value, psi);
    let utxo_b = Utxo::new_mint([bob_note.clone(), Note::padding_note()]);
    let proof_b = utxo_b.prove().unwrap();
    let tx_b_err = server.transaction(&proof_b).await.unwrap_err();
    assert_eq!(
        tx_b_err.get("error").unwrap().get("reason").unwrap(),
        &serde_json::Value::String("mint-hash-already-exists".to_owned())
    );

    // Check only one note was minted
    let alice_element = server.element(alice_note.commitment()).await.unwrap();
    assert_eq!(alice_element.element, alice_note.commitment());

    let bob_element_err = server.element(bob_note.commitment()).await.unwrap_err();
    assert_eq!(
        bob_element_err.get("error").unwrap().get("reason").unwrap(),
        &serde_json::Value::String("element-not-found".to_owned())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn query_transactions() {
    let eth_node = EthNode::default().run_and_deploy().await;
    let server = Rc::new(
        Server::setup_and_wait(ServerConfig::single_node(false), Arc::clone(&eth_node)).await,
    );
    let rollup = rollup_contract(server.rollup_contract_addr, &eth_node).await;
    let usdc = usdc_contract(&rollup, &eth_node).await;

    let alice_pk = Element::new(0xA11CE);
    let alice_address = hash_merge([alice_pk, Element::ZERO]);
    let mut notes = vec![];
    for value in [50u64, 100] {
        let (alice_note, eth_tx, tx) = mint(
            &rollup,
            &usdc,
            &server,
            alice_address,
            Element::from(value),
            Element::from(value),
        );
        eth_tx.await.unwrap();
        let tx = tx.await.unwrap();
        notes.push((alice_note, tx));
    }

    for note in &notes {
        let resp = server.get_transaction(note.1.txn_hash).await.unwrap();
        assert!(resp.txn.time > 1);
        // assert!(resp.txn.proof.leaves().contains(&(note.0.commitment())));

        let not_found = server
            .get_transaction(CryptoHash::new([0; 32]))
            .await
            .unwrap_err();
        assert_eq!(
            not_found.get("error").unwrap().get("reason").unwrap(),
            &serde_json::Value::String("txn-not-found".to_owned())
        );
    }

    {
        let resp = server.list_transactions(&Default::default()).await.unwrap();
        // Latest transaction should be first
        assert_eq!(resp.txns.len(), 2);
        // assert!(resp.txns[0]
        //     .proof
        //     .leaves()
        //     .contains(&(notes[1].0.commitment())));
        // assert!(resp.txns[1]
        //     .proof
        //     .leaves()
        //     .contains(&(notes[0].0.commitment())));
    }

    {
        // Oldest to newest transaction list
        let resp = server
            .list_transactions(&ListTxnsQuery {
                limit: Some(1),
                order: Some(ListTxnOrder::OldestToNewest),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(resp.txns.len(), 1);
        // assert!(resp.txns[0]
        //     .proof
        //     .leaves()
        //     .contains(&(notes[0].0.commitment())));

        // Next page
        let resp = server
            .list_transactions(&ListTxnsQuery {
                cursor: Some(CursorChoice::After(*resp.cursor.after.unwrap()).opaque()),
                order: Some(ListTxnOrder::OldestToNewest),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(resp.txns.len(), 1);
        // assert!(resp.txns[0]
        //     .proof
        //     .leaves()
        //     .contains(&(notes[1].0.commitment())));

        // Previous page
        let resp = server
            .list_transactions(&ListTxnsQuery {
                cursor: Some(CursorChoice::Before(*resp.cursor.before.unwrap()).opaque()),
                order: Some(ListTxnOrder::OldestToNewest),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(resp.txns.len(), 1);
        // assert!(resp.txns[0]
        //     .proof
        //     .leaves()
        //     .contains(&(notes[0].0.commitment())));

        // Previous page again should return nothing
        let resp = server
            .list_transactions(&ListTxnsQuery {
                cursor: Some(CursorChoice::Before(*resp.cursor.before.unwrap()).opaque()),
                order: Some(ListTxnOrder::OldestToNewest),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(resp.txns.len(), 0);
    }

    {
        let resp = server
            .list_transactions(&ListTxnsQuery {
                limit: Some(1),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(resp.txns.len(), 1);
        // assert!(resp.txns[0]
        //     .proof
        //     .leaves()
        //     .contains(&(notes[1].0.commitment())));

        // Next page
        let resp = server
            .list_transactions(&ListTxnsQuery {
                cursor: Some(CursorChoice::After(*resp.cursor.after.unwrap()).opaque()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(resp.txns.len(), 1);
        // assert!(resp.txns[0]
        //     .proof
        //     .leaves()
        //     .contains(&(notes[0].0.commitment())));

        // Previous page
        let resp = server
            .list_transactions(&ListTxnsQuery {
                cursor: Some(CursorChoice::Before(*resp.cursor.before.unwrap()).opaque()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(resp.txns.len(), 1);
        // assert!(resp.txns[0]
        //     .proof
        //     .leaves()
        //     .contains(&(notes[1].0.commitment())));

        // Previous page again should return nothing
        let resp_with_nothing = server
            .list_transactions(&ListTxnsQuery {
                cursor: Some(CursorChoice::Before(*resp.cursor.before.unwrap()).opaque()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(resp_with_nothing.txns.len(), 0);

        // Start polling and mint a new note
        let local_set = tokio::task::LocalSet::new();

        let server = Rc::clone(&server);

        let resp = local_set.spawn_local({
            let server = Rc::clone(&server);

            async move {
                server
                    .list_transactions(&ListTxnsQuery {
                        poll: Some(true),
                        cursor: Some(CursorChoice::Before(*resp.cursor.before.unwrap()).opaque()),
                        ..Default::default()
                    })
                    .await
            }
        });

        let mint = local_set.spawn_local(async move {
            let (new_note, eth_tx, tx) = mint(
                &rollup,
                &usdc,
                &server,
                alice_address,
                Element::from(150u64),
                Element::ZERO,
            );
            eth_tx.await.unwrap();
            let _tx = tx.await.unwrap();
            new_note
        });

        let (resp, _mint) = local_set
            .run_until(async { tokio::join!(resp, mint) })
            .await;

        // We should get the new note in the resp
        let resp = resp.unwrap().unwrap();
        assert_eq!(resp.txns.len(), 1);
        // assert_eq!(
        //     resp.txns[0].proof.leaves(),
        //     [
        //         Element::ZERO,
        //         Element::ZERO,
        //         mint.commitment(),
        //         Element::ZERO
        //     ]
        // );
    }

    expect_root_hash!(
        server,
        expect_test::expect![[r#"
            0xdc8048c64aab47beea9e3d82c8f7ed835748fd9cd975252e6fae7a7a489fe3a
        "#]]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn query_blocks() {
    let eth_node = EthNode::default().run_and_deploy().await;
    let server =
        Server::setup_and_wait(ServerConfig::single_node(false), Arc::clone(&eth_node)).await;
    let rollup = rollup_contract(server.rollup_contract_addr, &eth_node).await;
    let usdc = usdc_contract(&rollup, &eth_node).await;

    let alice_pk = Element::new(0xA11CE);
    let alice_address = hash_merge([alice_pk, Element::ZERO]);
    let mut notes = vec![];
    for value in [50u64, 100] {
        let (alice_note, eth_tx, tx) = mint(
            &rollup,
            &usdc,
            &server,
            alice_address,
            Element::from(value),
            Element::from(value),
        );
        eth_tx.await.unwrap();
        let tx = tx.await.unwrap();
        notes.push((alice_note, tx));
    }

    for (_note, txn_resp) in &notes {
        let resp = server
            .get_block(&txn_resp.height.to_string())
            .await
            .unwrap();
        assert_eq!(resp.block.content.header.height, txn_resp.height);
        // assert!(resp.block.content.state.txns[0]
        //     .proof
        //     .leaves()
        //     .contains(&(note.commitment())));

        let resp_by_hash = server
            .get_block(&resp.hash.0.encode_hex::<String>())
            .await
            .unwrap();
        assert_eq!(resp, resp_by_hash);
    }

    macro_rules! non_empty_blocks {
        ($blocks:expr) => {
            $blocks.filter(|b| !b.block.content.state.txns.is_empty())
        };
    }

    {
        let resp = server
            .list_blocks(&ListBlocksQuery {
                limit: Some(100),
                ..Default::default()
            })
            .await
            .unwrap();
        // Latest transaction should be first
        assert!(resp.blocks.len() >= notes.last().unwrap().1.height.0 as usize);
        // assert!(non_empty_blocks!(resp.blocks.iter())
        //     .next()
        //     .unwrap()
        //     .block
        //     .content
        //     .state
        //     .txns[0]
        //     .proof
        //     .leaves()
        //     .contains(&notes[1].0.commitment()));
        // assert!(non_empty_blocks!(resp.blocks.iter())
        //     .skip(1)
        //     .next()
        //     .unwrap()
        //     .block
        //     .content
        //     .state
        //     .txns[0]
        //     .proof
        //     .leaves()
        //     .contains(&notes[0].0.commitment()));
    }

    {
        // Lowest to highest block list
        let resp = server
            .list_blocks(&ListBlocksQuery {
                limit: Some(1),
                order: Some(ListBlocksOrder::LowestToHighest),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(resp.blocks.len(), 1);
        assert_eq!(resp.blocks[0].block.content.header.height, BlockHeight(1));

        // Next page
        let resp = server
            .list_blocks(&ListBlocksQuery {
                limit: Some(1),
                cursor: Some(CursorChoice::After(*resp.cursor.after.unwrap()).opaque()),
                order: Some(ListBlocksOrder::LowestToHighest),
            })
            .await
            .unwrap();
        assert_eq!(resp.blocks.len(), 1);
        assert_eq!(resp.blocks[0].block.content.header.height, BlockHeight(2));

        // Previous page
        let resp = server
            .list_blocks(&ListBlocksQuery {
                limit: Some(1),
                cursor: Some(CursorChoice::Before(*resp.cursor.before.unwrap()).opaque()),
                order: Some(ListBlocksOrder::LowestToHighest),
            })
            .await
            .unwrap();
        assert_eq!(resp.blocks.len(), 1);
        assert_eq!(resp.blocks[0].block.content.header.height, BlockHeight(1));

        // Previous page again should return nothing
        let resp = server
            .list_blocks(&ListBlocksQuery {
                limit: Some(1),
                cursor: Some(CursorChoice::Before(*resp.cursor.before.unwrap()).opaque()),
                order: Some(ListBlocksOrder::LowestToHighest),
            })
            .await
            .unwrap();
        assert_eq!(resp.blocks.len(), 0);
    }

    {
        let resp = server
            .list_blocks(&ListBlocksQuery {
                limit: Some(1),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(resp.blocks.len(), 1);
        assert!(resp.blocks[0].block.content.header.height >= notes[1].1.height);
        let first_page_height = resp.blocks[0].block.content.header.height;

        // Next page
        let resp = server
            .list_blocks(&ListBlocksQuery {
                limit: Some(1),
                cursor: Some(CursorChoice::After(*resp.cursor.after.unwrap()).opaque()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(resp.blocks.len(), 1);
        assert_eq!(
            resp.blocks[0].block.content.header.height,
            BlockHeight(first_page_height.0 - 1)
        );

        // Previous page
        let resp = server
            .list_blocks(&ListBlocksQuery {
                limit: Some(1),
                cursor: Some(CursorChoice::Before(*resp.cursor.before.unwrap()).opaque()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(resp.blocks.len(), 1);
        assert_eq!(
            resp.blocks[0].block.content.header.height,
            first_page_height
        );

        // Previous page again should return nothing
        let resp_with_nothing = server
            .list_blocks(&ListBlocksQuery {
                cursor: Some(CursorChoice::Before(*resp.cursor.before.unwrap()).opaque()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(resp_with_nothing.blocks.len(), 0);

        // If we add a transaction and try again, we should get the new transaction
        let (_new_note, eth_tx, tx) = mint(
            &rollup,
            &usdc,
            &server,
            alice_address,
            Element::from(150u64),
            Element::ZERO,
        );
        eth_tx.await.unwrap();
        let _tx = tx.await.unwrap();

        let resp = server
            .list_blocks(&ListBlocksQuery {
                cursor: Some(CursorChoice::Before(*resp.cursor.before.unwrap()).opaque()),
                limit: Some(100),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(non_empty_blocks!(resp.blocks.iter()).count(), 1);

        // assert!(non_empty_blocks!(resp.blocks.iter())
        //     .next()
        //     .unwrap()
        //     .block
        //     .content
        //     .state
        //     .txns[0]
        //     .proof
        //     .leaves()
        //     .contains(&new_note.commitment()));
    }

    expect_root_hash!(
        server,
        expect_test::expect![[r#"
            0xdc8048c64aab47beea9e3d82c8f7ed835748fd9cd975252e6fae7a7a489fe3a
        "#]]
    );
}
