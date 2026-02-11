use anyhow::{bail, Context, Result};
use client_sdk::rest_client::{NodeApiClient, NodeApiHttpClient};
use element::Element;
use hash::hash_merge;
use hyli_modules::{
    bus::{BusClientSender, BusMessage, SharedMessageBus},
    module_bus_client, module_handle_messages,
    modules::Module,
};
use hyli_utxo_state::{state::HyliUtxoStateAction, zk::BorshableH256};
use sdk::{
    Blob, BlobData, BlobTransaction, ContractName, Identity, ProgramId, ProofData,
    ProofTransaction, TxHash, Verifier,
};
use tracing::{info, warn};
use zk_primitives::{InputNote, Note, Utxo, HYLI_BLOB_HASH_BYTE_LENGTH, HYLI_BLOB_LENGTH_BYTES};

use crate::{init::HYLI_UTXO_NOIR_VK, noir_prover::HyliUtxoProofJob, tx::FAUCET_IDENTITY_PREFIX};

pub const FAUCET_MINT_AMOUNT: u64 = 10;

#[derive(Clone, Debug)]
pub struct FaucetMintCommand {
    pub recipient_pubkey: Vec<u8>,
    pub amount: u64,
    pub note: Note,
}

impl BusMessage for FaucetMintCommand {}

#[derive(Clone, Debug)]
pub struct TransferCommand {
    pub input_notes: [InputNote; 2],
    pub output_notes: [Note; 2],
}

impl BusMessage for TransferCommand {}

/// Transfer command with pre-generated proof (client-side proving)
#[derive(Clone, Debug)]
pub struct TransferWithProofCommand {
    /// Raw proof bytes (without public inputs)
    pub proof: Vec<u8>,
    /// Public inputs as hex strings (733 field elements)
    pub public_inputs: Vec<String>,
    /// 128-byte blob data
    pub blob: [u8; 128],
    /// Output notes [recipient_note, change_note]
    pub output_notes: [Note; 2],
}

impl BusMessage for TransferWithProofCommand {}

pub fn build_note(recipient_address: Element, amount: u64) -> Note {
    let minted_value = Element::new(amount);
    Note::new(recipient_address, minted_value)
}

module_bus_client! {
pub struct FaucetBusClient {
    sender(HyliUtxoProofJob),
    receiver(FaucetMintCommand),
    receiver(TransferCommand),
    receiver(TransferWithProofCommand),
}
}

#[derive(Clone)]
pub struct FaucetAppContext {
    pub client: NodeApiHttpClient,
    pub utxo_contract_name: String,
    pub utxo_state_contract_name: String,
}

pub struct FaucetApp {
    bus: FaucetBusClient,
    client: NodeApiHttpClient,
    notes_root: Element,
    nullifier_root: Element,
    note_index: u64,
    utxo_contract_name: String,
    utxo_state_contract_name: String,
}

impl Module for FaucetApp {
    type Context = FaucetAppContext;

    async fn build(bus: SharedMessageBus, ctx: Self::Context) -> Result<Self> {
        let faucet_bus = FaucetBusClient::new_from_bus(bus.new_handle()).await;

        Ok(Self {
            bus: faucet_bus,
            client: ctx.client,
            notes_root: Element::ZERO,
            nullifier_root: Element::ZERO,
            note_index: 0,
            utxo_contract_name: ctx.utxo_contract_name,
            utxo_state_contract_name: ctx.utxo_state_contract_name,
        })
    }

    async fn run(&mut self) -> Result<()> {
        module_handle_messages! {
            on_self self,
            listen<FaucetMintCommand> cmd => {
                self.process_request(cmd).await?;
            }
            listen<TransferCommand> cmd => {
                self.process_transfer_request(cmd).await?;
            }
            listen<TransferWithProofCommand> cmd => {
                self.process_transfer_with_proof(cmd).await?;
            }
        };

        Ok(())
    }
}

impl FaucetApp {
    async fn process_request(&mut self, request: FaucetMintCommand) -> Result<()> {
        let (blob_transaction, utxo) = self.build_transaction(&request.note)?;

        let tx_hash = self
            .client
            .send_tx_blob(blob_transaction.clone())
            .await
            .context("dispatching blob transaction")?;

        info!(%tx_hash, "Submitted hyli_utxo faucet transaction");

        if let Err(err) = self.enqueue_proof_job(&blob_transaction, &tx_hash, utxo) {
            warn!(error = %err, "failed to enqueue Noir proof job");
        }

        Ok(())
    }

    fn build_transaction(&mut self, note: &Note) -> Result<(BlobTransaction, Utxo)> {
        let utxo = Utxo::new_mint([note.clone(), Note::padding_note()]);

        let leaf_elements = utxo.leaf_elements();
        self.notes_root = leaf_elements[2];
        self.note_index = self.note_index.wrapping_add(1);

        let mut blob_bytes = vec![0u8; HYLI_BLOB_LENGTH_BYTES];
        let mut offset = 0usize;

        for commitment in &leaf_elements[0..2] {
            blob_bytes[offset..offset + HYLI_BLOB_HASH_BYTE_LENGTH]
                .copy_from_slice(&commitment.to_be_bytes());
            offset += HYLI_BLOB_HASH_BYTE_LENGTH;
        }

        let mut state_commitments = [BorshableH256::from([0u8; 32]); 4];

        for (index, commitment) in leaf_elements[2..].iter().enumerate() {
            state_commitments[index] = BorshableH256::from(commitment.to_be_bytes());
        }

        let mut nullifier_index = 2;
        for input in utxo.input_notes.iter() {
            let nullifier = hash_merge([input.note.psi, input.secret_key]);

            blob_bytes[offset..offset + HYLI_BLOB_HASH_BYTE_LENGTH]
                .copy_from_slice(&nullifier.to_be_bytes());
            offset += HYLI_BLOB_HASH_BYTE_LENGTH;

            self.nullifier_root = nullifier;

            if !input.note.is_padding_note() && nullifier_index < state_commitments.len() {
                state_commitments[nullifier_index] = BorshableH256::from(nullifier.to_be_bytes());
                nullifier_index += 1;
            }
        }

        let state_action: HyliUtxoStateAction = state_commitments;

        let contract_name = self.utxo_contract_name.clone();
        let identity = Identity(format!("{}@{}", FAUCET_IDENTITY_PREFIX, contract_name));
        let hyli_utxo_data = BlobData(blob_bytes);
        let state_blob_data = BlobData(
            borsh::to_vec(&state_action).expect("HyliUtxoStateAction serialization failed"),
        );
        let hyli_utxo_blob = Blob {
            contract_name: contract_name.clone().into(),
            data: hyli_utxo_data,
        };
        let state_blob = Blob {
            contract_name: ContractName(self.utxo_state_contract_name.clone()),
            data: state_blob_data,
        };
        let blob_transaction = BlobTransaction::new(identity, vec![state_blob, hyli_utxo_blob]);

        Ok((blob_transaction, utxo))
    }

    fn enqueue_proof_job(
        &mut self,
        blob_tx: &BlobTransaction,
        tx_hash: &TxHash,
        utxo: Utxo,
    ) -> Result<()> {
        let Some((blob_index, blob)) = blob_tx
            .blobs
            .iter()
            .enumerate()
            .find(|(_, blob)| blob.contract_name.0 == self.utxo_contract_name)
        else {
            bail!("hyli_utxo blob not found in transaction payload");
        };

        if blob.data.0.len() < HYLI_BLOB_LENGTH_BYTES {
            bail!(
                "hyli_utxo blob payload is {} bytes, expected {}",
                blob.data.0.len(),
                HYLI_BLOB_LENGTH_BYTES
            );
        }

        let mut payload = [0u8; HYLI_BLOB_LENGTH_BYTES];
        payload.copy_from_slice(&blob.data.0[..HYLI_BLOB_LENGTH_BYTES]);

        let job = HyliUtxoProofJob {
            tx_hash: tx_hash.clone(),
            identity: blob_tx.identity.clone(),
            utxo,
            blob: payload,
            tx_blob_count: blob_tx.blobs.len() as u32,
            blob_index: blob_index as u32,
        };

        self.bus
            .send(job)
            .context("broadcasting hyli_utxo proof job")?;

        Ok(())
    }

    async fn process_transfer_request(&mut self, cmd: TransferCommand) -> Result<()> {
        let (blob_tx, utxo) =
            self.build_transfer_transaction(&cmd.input_notes, &cmd.output_notes)?;

        let tx_hash = self
            .client
            .send_tx_blob(blob_tx.clone())
            .await
            .context("dispatching transfer blob transaction")?;

        info!(%tx_hash, "Submitted transfer transaction");

        if let Err(err) = self.enqueue_proof_job(&blob_tx, &tx_hash, utxo) {
            warn!(error = %err, "failed to enqueue transfer proof job");
        }

        Ok(())
    }

    /// Process a transfer with pre-generated proof (client-side proving)
    async fn process_transfer_with_proof(&mut self, cmd: TransferWithProofCommand) -> Result<()> {
        // Build blob transaction from provided blob data and output notes
        let blob_tx = self.build_proved_transfer_blob(&cmd)?;

        // Submit blob transaction
        let tx_hash = self
            .client
            .send_tx_blob(blob_tx.clone())
            .await
            .context("dispatching proved transfer blob transaction")?;

        info!(%tx_hash, "Submitted proved transfer blob transaction");

        // Build and submit proof transaction
        // Convert public inputs from hex strings to bytes
        let public_inputs_bytes: Vec<u8> = cmd
            .public_inputs
            .iter()
            .flat_map(|hex_str| {
                let normalized = hex_str.strip_prefix("0x").unwrap_or(hex_str);
                hex::decode(normalized).unwrap_or_else(|_| vec![0u8; 32])
            })
            .collect();

        // Combine public inputs and proof
        let mut proof_with_inputs = public_inputs_bytes;
        proof_with_inputs.extend_from_slice(&cmd.proof);

        // Build and submit proof transaction to the node
        let proof_tx = ProofTransaction {
            contract_name: ContractName(self.utxo_contract_name.clone()),
            program_id: ProgramId(HYLI_UTXO_NOIR_VK.to_vec()),
            verifier: Verifier(sdk::verifiers::NOIR.to_string()),
            proof: ProofData(proof_with_inputs),
        };

        self.client
            .send_tx_proof(proof_tx)
            .await
            .context("submitting client-generated proof to node")?;

        info!(%tx_hash, "Submitted client-generated proof transaction");

        Ok(())
    }

    /// Build a blob transaction for a proved transfer (without computing nullifiers on server)
    fn build_proved_transfer_blob(
        &self,
        cmd: &TransferWithProofCommand,
    ) -> Result<BlobTransaction> {
        // Extract nullifiers from the blob (bytes 64-128 contain nullifier_0 and nullifier_1)
        let mut nullifier_0 = [0u8; 32];
        let mut nullifier_1 = [0u8; 32];
        nullifier_0.copy_from_slice(&cmd.blob[64..96]);
        nullifier_1.copy_from_slice(&cmd.blob[96..128]);

        // Build state action: [created_0, created_1, nullified_0, nullified_1]
        let mut state_commitments = [BorshableH256::from([0u8; 32]); 4];

        // Output commitments (created)
        state_commitments[0] = BorshableH256::from(cmd.output_notes[0].commitment().to_be_bytes());
        state_commitments[1] = BorshableH256::from(cmd.output_notes[1].commitment().to_be_bytes());

        // Nullifiers from blob
        state_commitments[2] = BorshableH256::from(nullifier_0);
        state_commitments[3] = BorshableH256::from(nullifier_1);

        let state_action: HyliUtxoStateAction = state_commitments;

        let contract_name = self.utxo_contract_name.clone();
        let identity = Identity(format!("transfer@{}", contract_name));
        let hyli_utxo_data = BlobData(cmd.blob.to_vec());
        let state_blob_data = BlobData(
            borsh::to_vec(&state_action).expect("HyliUtxoStateAction serialization failed"),
        );
        let hyli_utxo_blob = Blob {
            contract_name: contract_name.clone().into(),
            data: hyli_utxo_data,
        };
        let state_blob = Blob {
            contract_name: ContractName(self.utxo_state_contract_name.clone()),
            data: state_blob_data,
        };
        let blob_transaction = BlobTransaction::new(identity, vec![state_blob, hyli_utxo_blob]);

        Ok(blob_transaction)
    }

    fn build_transfer_transaction(
        &mut self,
        input_notes: &[InputNote; 2],
        output_notes: &[Note; 2],
    ) -> Result<(BlobTransaction, Utxo)> {
        let utxo = Utxo::new_send(input_notes.clone(), output_notes.clone());

        let leaf_elements = utxo.leaf_elements();

        // Build 128-byte blob: [input_commit_0, input_commit_1, nullifier_0, nullifier_1]
        let mut blob_bytes = vec![0u8; HYLI_BLOB_LENGTH_BYTES];
        let mut offset = 0usize;

        // Input commitments (first 64 bytes)
        for commitment in &leaf_elements[0..2] {
            blob_bytes[offset..offset + HYLI_BLOB_HASH_BYTE_LENGTH]
                .copy_from_slice(&commitment.to_be_bytes());
            offset += HYLI_BLOB_HASH_BYTE_LENGTH;
        }

        // Nullifiers (next 64 bytes)
        for input in input_notes.iter() {
            let nullifier = hash_merge([input.note.psi, input.secret_key]);
            blob_bytes[offset..offset + HYLI_BLOB_HASH_BYTE_LENGTH]
                .copy_from_slice(&nullifier.to_be_bytes());
            offset += HYLI_BLOB_HASH_BYTE_LENGTH;
        }

        // Build state action: [created_0, created_1, nullified_0, nullified_1]
        let mut state_commitments = [BorshableH256::from([0u8; 32]); 4];

        // Output commitments (created)
        state_commitments[0] = BorshableH256::from(output_notes[0].commitment().to_be_bytes());
        state_commitments[1] = BorshableH256::from(output_notes[1].commitment().to_be_bytes());

        // Nullifiers
        for (i, input) in input_notes.iter().enumerate() {
            if !input.note.is_padding_note() {
                let nullifier = hash_merge([input.note.psi, input.secret_key]);
                state_commitments[2 + i] = BorshableH256::from(nullifier.to_be_bytes());
            }
        }

        let state_action: HyliUtxoStateAction = state_commitments;

        let contract_name = self.utxo_contract_name.clone();
        let identity = Identity(format!("transfer@{}", contract_name));
        let hyli_utxo_data = BlobData(blob_bytes);
        let state_blob_data = BlobData(
            borsh::to_vec(&state_action).expect("HyliUtxoStateAction serialization failed"),
        );
        let hyli_utxo_blob = Blob {
            contract_name: contract_name.clone().into(),
            data: hyli_utxo_data,
        };
        let state_blob = Blob {
            contract_name: ContractName(self.utxo_state_contract_name.clone()),
            data: state_blob_data,
        };
        let blob_transaction = BlobTransaction::new(identity, vec![state_blob, hyli_utxo_blob]);

        Ok((blob_transaction, utxo))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hyli_utxo_state_client::HyliUtxoStateExecutor, noir_prover::HyliUtxoNoirProver};
    use barretenberg::{Prove, Verify};
    use client_sdk::{
        helpers::{test::MockProver, ClientSdkProver},
        rest_client::test::NodeApiMockClient,
        transaction_builder::TxExecutorHandler,
    };
    use element::Element;
    use hyli_modules::{
        bus::{dont_use_this, metrics::BusMetrics, SharedMessageBus},
        modules::prover::{AutoProver, AutoProverCtx},
    };
    use hyli_utxo_state::{state::HyliUtxoStateAction, zk::BorshableH256};
    use sdk::hyli_model_utils::TimestampMs;
    use sdk::{
        AggregateSignature, Blob, BlobIndex, BlobTransaction, Block, BlockHeight, BlockStakingData,
        Calldata, ConsensusProposal, ConsensusProposalHash, Contract, ContractName,
        DataProposalHash, NodeStateBlock, NodeStateEvent, ProgramId, StatefulEvent, StatefulEvents,
        TimeoutWindow, Transaction, TxContext, TxHash, TxId, ValidatorPublicKey, Verifier,
        HYLI_TESTNET_CHAIN_ID,
    };
    use sdk::{Hashed, LaneId, SignedBlock};
    use sha3::{Digest, Sha3_256};
    use std::{collections::BTreeMap, sync::Arc};
    use tempfile::tempdir;
    use tokio::time::{sleep, timeout, Duration};
    use zk_primitives::HyliUtxo;

    const TEST_UTXO_CONTRACT_NAME: &str = "hyli_utxo";
    const TEST_UTXO_STATE_CONTRACT_NAME: &str = "hyli-utxo-state";

    fn deterministic_address(label: &str) -> Element {
        let mut hasher = Sha3_256::new();
        hasher.update(label.as_bytes());
        let digest: [u8; 32] = hasher.finalize().into();
        Element::from_be_bytes(digest)
    }

    fn find_hyli_blob(blobs: &[Blob]) -> (usize, &[u8]) {
        blobs
            .iter()
            .enumerate()
            .find(|(_, blob)| blob.contract_name.0 == TEST_UTXO_CONTRACT_NAME)
            .map(|(idx, blob)| (idx, blob.data.0.as_slice()))
            .expect("hyli_utxo blob not found")
    }

    fn build_node_state_block(
        blob_tx: BlobTransaction,
        contract: Contract,
        block_height: u64,
    ) -> NodeStateBlock {
        let tx_hash = blob_tx.hashed();
        let tx_id = TxId(
            DataProposalHash(format!("dp-{block_height}")),
            tx_hash.clone(),
        );
        let tx_ctx = Arc::new(TxContext {
            lane_id: LaneId(ValidatorPublicKey(vec![0u8; 32])),
            block_hash: ConsensusProposalHash(format!("block-{block_height}")),
            block_height: BlockHeight(block_height),
            timestamp: TimestampMs::ZERO,
            chain_id: HYLI_TESTNET_CHAIN_ID,
        });

        let sequenced_tx = blob_tx.clone();

        let stateful_events = StatefulEvents {
            events: vec![
                (
                    tx_id.clone(),
                    StatefulEvent::ContractUpdate(contract.name.clone(), contract.clone()),
                ),
                (
                    tx_id.clone(),
                    StatefulEvent::SequencedTx(sequenced_tx, tx_ctx),
                ),
            ],
        };

        let signed_block = SignedBlock {
            data_proposals: Vec::new(),
            consensus_proposal: ConsensusProposal {
                slot: block_height,
                parent_hash: ConsensusProposalHash("parent".into()),
                cut: Vec::new(),
                staking_actions: Vec::new(),
                timestamp: TimestampMs::ZERO,
            },
            certificate: AggregateSignature::default(),
        };

        let transaction: Transaction = blob_tx.into();

        let block = Block {
            parent_hash: ConsensusProposalHash("parent".into()),
            hash: ConsensusProposalHash(format!("hash-{block_height}")),
            block_height: BlockHeight(block_height),
            block_timestamp: TimestampMs::ZERO,
            txs: vec![(tx_id, transaction)],
            dp_parent_hashes: BTreeMap::new(),
            lane_ids: BTreeMap::new(),
            successful_txs: Vec::new(),
            failed_txs: Vec::new(),
            timed_out_txs: Vec::new(),
            dropped_duplicate_txs: Vec::new(),
            blob_proof_outputs: Vec::new(),
            verified_blobs: Vec::new(),
            registered_contracts: BTreeMap::new(),
            deleted_contracts: BTreeMap::new(),
            updated_states: BTreeMap::new(),
            updated_program_ids: BTreeMap::new(),
            updated_timeout_windows: BTreeMap::new(),
            transactions_events: BTreeMap::new(),
        };

        NodeStateBlock {
            signed_block: Arc::new(signed_block),
            parsed_block: Arc::new(block),
            staking_data: Arc::new(BlockStakingData::default()),
            stateful_events: Arc::new(stateful_events),
        }
    }

    #[tokio::test]
    async fn hyli_utxo_blob_matches_expected_payload() {
        let bus = SharedMessageBus::new(BusMetrics::global("test".to_string()));
        let context = FaucetAppContext {
            client: NodeApiHttpClient::new("http://localhost:19999".to_string())
                .expect("client init"),
            utxo_contract_name: TEST_UTXO_CONTRACT_NAME.to_string(),
            utxo_state_contract_name: TEST_UTXO_STATE_CONTRACT_NAME.to_string(),
        };

        let mut app = FaucetApp::build(bus, context)
            .await
            .expect("building faucet app");

        let recipient_address = deterministic_address("alice");

        let note = build_note(recipient_address, FAUCET_MINT_AMOUNT);

        let (blob_tx, utxo) = app.build_transaction(&note).expect("build transaction");

        let (blob_index, payload) = find_hyli_blob(&blob_tx.blobs);

        let mut blob_bytes = [0u8; HYLI_BLOB_LENGTH_BYTES];
        blob_bytes.copy_from_slice(&payload[..HYLI_BLOB_LENGTH_BYTES]);

        let identity = blob_tx.identity.0.clone();
        let tx_hash_placeholder = "0".repeat(64);

        let job = HyliUtxoProofJob {
            tx_hash: TxHash(tx_hash_placeholder.clone()),
            identity: blob_tx.identity.clone(),
            utxo: utxo.clone(),
            blob: blob_bytes,
            tx_blob_count: blob_tx.blobs.len() as u32,
            blob_index: blob_index as u32,
        };

        let hyli_utxo = HyliUtxoNoirProver::build_hyli_utxo(TEST_UTXO_CONTRACT_NAME, &job)
            .expect("build hyli utxo");

        assert_eq!(hyli_utxo.identity_len as usize, identity.len());
        assert_eq!(
            hyli_utxo.blob_contract_name_len as usize,
            TEST_UTXO_CONTRACT_NAME.len(),
            "contract name length should match expected contract name length"
        );
        assert_eq!(hyli_utxo.identity.len(), 256);
        assert_eq!(hyli_utxo.blob_contract_name.len(), 256);

        let expected_blob = hyli_utxo.expected_blob();

        assert_eq!(expected_blob.as_slice(), &blob_bytes);

        let expected_nullifiers = hyli_utxo.nullifier_commitments();
        for (index, commitment) in expected_nullifiers.iter().enumerate() {
            let start = 64 + index * 32;
            let mut emitted = [0u8; 32];
            emitted.copy_from_slice(&blob_bytes[start..start + 32]);
            assert_eq!(commitment.to_be_bytes(), emitted);
        }
    }

    #[tokio::test]
    async fn hyli_utxo_state_action_orders_commitments() {
        let bus = SharedMessageBus::new(BusMetrics::global("test".to_string()));
        let context = FaucetAppContext {
            client: NodeApiHttpClient::new("http://localhost:19999".to_string())
                .expect("client init"),
            utxo_contract_name: TEST_UTXO_CONTRACT_NAME.to_string(),
            utxo_state_contract_name: TEST_UTXO_STATE_CONTRACT_NAME.to_string(),
        };

        let mut app = FaucetApp::build(bus, context)
            .await
            .expect("building faucet app");

        let recipient_address = deterministic_address("state-order-test");

        let (blob_tx, recipient_note, utxo) = app
            .build_transaction(recipient_address, FAUCET_MINT_AMOUNT)
            .expect("build transaction");

        let (state_index, state_blob) = blob_tx
            .blobs
            .iter()
            .enumerate()
            .find(|(_, blob)| blob.contract_name.0 == TEST_UTXO_STATE_CONTRACT_NAME)
            .expect("state blob present");

        let state_action: HyliUtxoStateAction =
            borsh::from_slice(&state_blob.data.0).expect("decode state action");

        let (created, nullified) = state_action.split_at(2);

        let expected_created = BorshableH256::from(recipient_note.commitment().to_be_bytes());
        assert_eq!(&created[0], &expected_created);
        let expected_created_second =
            BorshableH256::from(utxo.output_notes[1].commitment().to_be_bytes());
        assert_eq!(&created[1], &expected_created_second);

        let expected_nullifiers: Vec<BorshableH256> = utxo
            .input_notes
            .iter()
            .filter(|input| !input.note.is_padding_note())
            .map(|input| hash_merge([input.note.psi, input.secret_key]))
            .map(|value| BorshableH256::from(value.to_be_bytes()))
            .collect();

        let actual_nullifiers: Vec<BorshableH256> = nullified
            .iter()
            .copied()
            .filter(|commitment| {
                let bytes: [u8; 32] = commitment.0.into();
                bytes != [0u8; 32]
            })
            .collect();

        assert_eq!(actual_nullifiers, expected_nullifiers);

        // Ensure executor accepts the state blob without duplicate errors.
        let mut executor = HyliUtxoStateExecutor::default();
        let metadata = executor
            .build_commitment_metadata(state_blob)
            .expect("build commitment metadata");
        assert!(
            !metadata.is_empty(),
            "commitment metadata should not be empty"
        );

        let calldata = Calldata {
            tx_hash: TxHash("test".into()),
            identity: blob_tx.identity.clone(),
            blobs: blob_tx.blobs.clone().into(),
            tx_blob_count: blob_tx.blobs.len(),
            index: BlobIndex(state_index),
            tx_ctx: None,
            private_input: Vec::new(),
        };

        executor
            .handle(&calldata)
            .expect("applying state blob should succeed");
    }

    #[tokio::test]
    async fn hyli_utxo_noir_proof_verifies() {
        if std::process::Command::new("bb")
            .arg("--version")
            .status()
            .is_err()
        {
            eprintln!(
                "Skipping hyli_utxo proof verification test because 'bb' binary is not available"
            );
            return;
        }

        let bus = SharedMessageBus::new(BusMetrics::global("test".to_string()));
        let context = FaucetAppContext {
            client: NodeApiHttpClient::new("http://localhost:19999".to_string())
                .expect("client init"),
            utxo_contract_name: TEST_UTXO_CONTRACT_NAME.to_string(),
            utxo_state_contract_name: TEST_UTXO_STATE_CONTRACT_NAME.to_string(),
        };

        let mut app = FaucetApp::build(bus, context)
            .await
            .expect("building faucet app");

        let recipient_address = deterministic_address("noir-proof-test");

        let (blob_tx, _note, utxo) = app
            .build_transaction(recipient_address, FAUCET_MINT_AMOUNT)
            .expect("build transaction");

        let (blob_index, payload) = find_hyli_blob(&blob_tx.blobs);

        let mut blob_bytes = [0u8; HYLI_BLOB_LENGTH_BYTES];
        blob_bytes.copy_from_slice(&payload[..HYLI_BLOB_LENGTH_BYTES]);

        let job = HyliUtxoProofJob {
            tx_hash: TxHash("0".repeat(64)),
            identity: blob_tx.identity.clone(),
            utxo,
            blob: blob_bytes,
            tx_blob_count: blob_tx.blobs.len() as u32,
            blob_index: blob_index as u32,
        };

        let hyli_utxo = HyliUtxoNoirProver::build_hyli_utxo(TEST_UTXO_CONTRACT_NAME, &job)
            .expect("build hyli utxo");

        let proof = hyli_utxo.prove().expect("generate hyli_utxo proof");

        proof.verify().expect("verify hyli_utxo proof");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn auto_prover_emits_state_proof_after_mint() -> Result<()> {
        let shared_bus = SharedMessageBus::new(BusMetrics::global("autoprover-test".to_string()));

        let api_client = Arc::new(NodeApiMockClient::new());
        let mock_prover = Arc::new(MockProver {});

        let default_executor = HyliUtxoStateExecutor::default();
        let initial_commitment = default_executor.get_state_commitment();
        let program_id = ProgramId("MockProver".as_bytes().to_vec());
        let verifier: Verifier = "mock".into();

        let contract_name = ContractName::new(TEST_UTXO_STATE_CONTRACT_NAME);
        let contract = Contract {
            name: contract_name.clone(),
            program_id: program_id.clone(),
            state: initial_commitment.clone(),
            verifier: verifier.clone(),
            timeout_window: TimeoutWindow::NoTimeout,
        };
        api_client.add_contract(contract.clone());

        let data_dir = tempdir().expect("tempdir");
        let prover_arc: Arc<dyn ClientSdkProver<Vec<Calldata>> + Send + Sync> = mock_prover.clone();
        let node_arc: Arc<dyn NodeApiClient + Send + Sync> = api_client.clone();

        let ctx = Arc::new(AutoProverCtx {
            data_directory: data_dir.path().to_path_buf(),
            prover: prover_arc,
            contract_name: contract_name.clone(),
            node: node_arc,
            api: None,
            default_state: default_executor.clone(),
            buffer_blocks: 0,
            max_txs_per_proof: 4,
            tx_working_window_size: 1,
        });

        let auto_prover = AutoProver::<HyliUtxoStateExecutor>::build(shared_bus.new_handle(), ctx)
            .await
            .expect("build autoprover");

        let auto_prover_handle = tokio::spawn(async move {
            let mut prover = auto_prover;
            let _ = prover.run().await;
        });

        sleep(Duration::from_millis(50)).await;

        let faucet_bus = SharedMessageBus::new(BusMetrics::global("faucet-autoprover".to_string()));
        let faucet_context = FaucetAppContext {
            client: NodeApiHttpClient::new("http://localhost:19999".to_string())
                .expect("client init"),
            utxo_contract_name: TEST_UTXO_CONTRACT_NAME.to_string(),
            utxo_state_contract_name: TEST_UTXO_STATE_CONTRACT_NAME.to_string(),
        };
        let mut faucet = FaucetApp::build(faucet_bus, faucet_context)
            .await
            .expect("build faucet app");
        let recipient_address = deterministic_address("autoprover-test");
        let (blob_tx, _, _) = faucet
            .build_transaction(recipient_address, FAUCET_MINT_AMOUNT)
            .expect("build transaction");
        api_client.set_block_height(BlockHeight(0));
        let block = build_node_state_block(blob_tx.clone(), contract.clone(), 0);
        let has_state_blob = block
            .stateful_events
            .events
            .iter()
            .any(|(_, event)| match event {
                StatefulEvent::SequencedTx(tx, _) => tx
                    .blobs
                    .iter()
                    .any(|blob| blob.contract_name == contract_name),
                _ => false,
            });
        assert!(has_state_blob, "block must contain hyli_utxo_state blob");
        let sender = dont_use_this::get_sender::<NodeStateEvent>(&shared_bus).await;
        sender
            .send(NodeStateEvent::NewBlock(block))
            .expect("send node state event");
        let proof = timeout(Duration::from_secs(5), async {
            loop {
                if let Some(proof) = api_client
                    .pending_proofs
                    .lock()
                    .expect("lock pending proofs")
                    .first()
                    .cloned()
                {
                    break proof;
                }
                sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("proof timeout");

        assert_eq!(proof.contract_name, contract_name);
        assert!(
            !proof.proof.0.is_empty(),
            "proof payload should not be empty"
        );

        auto_prover_handle.abort();
        let _ = auto_prover_handle.await;

        Ok(())
    }
}
