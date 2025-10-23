use anyhow::{bail, Context, Result};
use client_sdk::rest_client::{NodeApiClient, NodeApiHttpClient};
use element::Element;
use hash::hash_merge;
use hyli_modules::{
    bus::{command_response::Query, BusClientSender, SharedMessageBus},
    module_bus_client, module_handle_messages,
    modules::Module,
};
use hyli_utxo_state::{state::HyliUtxoStateAction, zk::BorshableH256};
use sdk::{Blob, BlobData, BlobTransaction, ContractName, Identity, TxHash};
use tracing::{info, warn};
use zk_primitives::{Note, Utxo, HYLI_BLOB_HASH_BYTE_LENGTH, HYLI_BLOB_LENGTH_BYTES};

use crate::{
    init::HYLI_UTXO_STATE_CONTRACT_NAME,
    keys::KeyMaterial,
    noir_prover::HyliUtxoProofJob,
    tx::{FAUCET_IDENTITY_PREFIX, HYLI_UTXO_CONTRACT_NAME},
};

pub const FAUCET_MINT_AMOUNT: u64 = 10;

#[derive(Clone, Debug)]
pub struct FaucetMintCommand {
    pub key_material: KeyMaterial,
    pub amount: u64,
}

#[derive(Clone, Debug)]
pub struct FaucetMintResult {
    pub note: Note,
}

module_bus_client! {
pub struct FaucetBusClient {
    sender(HyliUtxoProofJob),
    receiver(Query<FaucetMintCommand, FaucetMintResult>),
}
}

#[derive(Clone)]
pub struct FaucetAppContext {
    pub client: NodeApiHttpClient,
}

pub struct FaucetApp {
    bus: FaucetBusClient,
    client: NodeApiHttpClient,
    notes_root: Element,
    nullifier_root: Element,
    note_index: u64,
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
        })
    }

    async fn run(&mut self) -> Result<()> {
        module_handle_messages! {
            on_self self,
            command_response<FaucetMintCommand, FaucetMintResult> cmd => {
                self.process_request(cmd.clone()).await
            }
        };

        Ok(())
    }
}

impl FaucetApp {
    async fn process_request(&mut self, request: FaucetMintCommand) -> Result<FaucetMintResult> {
        let (blob_transaction, recipient_note, utxo) =
            self.build_transaction(&request.key_material, request.amount)?;

        let tx_hash = self
            .client
            .send_tx_blob(blob_transaction.clone())
            .await
            .context("dispatching blob transaction")?;

        info!(%tx_hash, "Submitted hyli_utxo faucet transaction");

        if let Err(err) = self.enqueue_proof_job(&blob_transaction, &tx_hash, utxo) {
            warn!(error = %err, "failed to enqueue Noir proof job");
        }

        Ok(FaucetMintResult {
            note: recipient_note,
        })
    }

    fn build_transaction(
        &mut self,
        key_material: &KeyMaterial,
        amount: u64,
    ) -> Result<(BlobTransaction, Note, Utxo)> {
        let private_key = Element::from_be_bytes(key_material.private_key);
        let minted_value = Element::new(amount);

        let recipient_note = Note::new(private_key, minted_value);
        let utxo = Utxo::new_mint([recipient_note.clone(), Note::padding_note()]);

        let leaf_elements = utxo.leaf_elements();
        self.notes_root = leaf_elements[2];
        self.note_index = self.note_index.wrapping_add(1);

        let mut blob_bytes = vec![0u8; HYLI_BLOB_LENGTH_BYTES];
        let mut offset = 0usize;

        let mut commitments = Vec::with_capacity(4);

        for commitment in &leaf_elements[0..2] {
            blob_bytes[offset..offset + HYLI_BLOB_HASH_BYTE_LENGTH]
                .copy_from_slice(&commitment.to_be_bytes());
            offset += HYLI_BLOB_HASH_BYTE_LENGTH;
            commitments.push(BorshableH256::from(commitment.to_be_bytes()));
        }

        for input in utxo.input_notes.iter() {
            let nullifier = hash_merge([input.note.psi, input.secret_key]);

            blob_bytes[offset..offset + HYLI_BLOB_HASH_BYTE_LENGTH]
                .copy_from_slice(&nullifier.to_be_bytes());
            offset += HYLI_BLOB_HASH_BYTE_LENGTH;

            self.nullifier_root = nullifier;

            commitments.push(BorshableH256::from(nullifier.to_be_bytes()));
        }

        let state_action: HyliUtxoStateAction = commitments
            .try_into()
            .expect("expected exactly four commitments for state action");

        let contract_name = HYLI_UTXO_CONTRACT_NAME.to_string();
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
            contract_name: ContractName(HYLI_UTXO_STATE_CONTRACT_NAME.to_string()),
            data: state_blob_data,
        };
        let blob_transaction = BlobTransaction::new(identity, vec![state_blob, hyli_utxo_blob]);

        Ok((blob_transaction, recipient_note, utxo))
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
            .find(|(_, blob)| blob.contract_name.0 == HYLI_UTXO_CONTRACT_NAME)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{keys::derive_key_material, noir_prover::HyliUtxoNoirProver};
    use barretenberg::{Prove, Verify};
    use hyli_modules::bus::metrics::BusMetrics;
    use sdk::{Blob, TxHash};
    use zk_primitives::HyliUtxo;

    fn find_hyli_blob(blobs: &[Blob]) -> (usize, &[u8]) {
        blobs
            .iter()
            .enumerate()
            .find(|(_, blob)| blob.contract_name.0 == HYLI_UTXO_CONTRACT_NAME)
            .map(|(idx, blob)| (idx, blob.data.0.as_slice()))
            .expect("hyli_utxo blob not found")
    }

    #[tokio::test]
    async fn hyli_utxo_blob_matches_expected_payload() {
        let bus = SharedMessageBus::new(BusMetrics::global("test".to_string()));
        let context = FaucetAppContext {
            client: NodeApiHttpClient::new("http://localhost:19999".to_string())
                .expect("client init"),
        };

        let mut app = FaucetApp::build(bus, context)
            .await
            .expect("building faucet app");

        let key_material = derive_key_material("alice").expect("key material");

        let (blob_tx, _note, utxo) = app
            .build_transaction(&key_material, FAUCET_MINT_AMOUNT)
            .expect("build transaction");

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

        let hyli_utxo = HyliUtxoNoirProver::build_hyli_utxo(&job).expect("build hyli utxo");

        assert_eq!(hyli_utxo.identity_len as usize, identity.len());
        assert_eq!(
            hyli_utxo.blob_contract_name_len as usize,
            hyli_utxo.blob_contract_name.len(),
            "contract name length should match string length"
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
        };

        let mut app = FaucetApp::build(bus, context)
            .await
            .expect("building faucet app");

        let key_material = derive_key_material("noir-proof-test").expect("key material");

        let (blob_tx, _note, utxo) = app
            .build_transaction(&key_material, FAUCET_MINT_AMOUNT)
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

        let hyli_utxo = HyliUtxoNoirProver::build_hyli_utxo(&job).expect("build hyli utxo");

        let proof = hyli_utxo.prove().expect("generate hyli_utxo proof");

        proof.verify().expect("verify hyli_utxo proof");
    }
}
