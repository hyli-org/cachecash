use anyhow::{Context, Result};
use client_sdk::rest_client::{NodeApiClient, NodeApiHttpClient};
use element::Element;
use hash::hash_merge;
use hyli_modules::{
    bus::{command_response::Query, SharedMessageBus},
    module_bus_client, module_handle_messages,
    modules::Module,
};
use hyli_utxo_state::{state::HyliUtxoStateAction, zk::BorshableH256};
use sdk::{Blob, BlobData, BlobTransaction, ContractName, Identity};
use tracing::info;
use zk_primitives::{Note, Utxo, HYLI_BLOB_HASH_BYTE_LENGTH, HYLI_BLOB_LENGTH_BYTES};

use crate::{
    init::HYLI_UTXO_STATE_CONTRACT_NAME,
    keys::KeyMaterial,
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
        Ok(Self {
            bus: FaucetBusClient::new_from_bus(bus.new_handle()).await,
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
        let (blob_transaction, recipient_note) =
            self.build_transaction(&request.key_material, request.amount)?;

        let tx_hash = self
            .client
            .send_tx_blob(blob_transaction.clone())
            .await
            .context("dispatching blob transaction")?;

        info!(%tx_hash, "Submitted hyli_utxo faucet transaction");

        Ok(FaucetMintResult {
            note: recipient_note,
        })
    }

    fn build_transaction(
        &mut self,
        key_material: &KeyMaterial,
        amount: u64,
    ) -> Result<(BlobTransaction, Note)> {
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
            let nullifier = if input.secret_key.is_zero() && input.note.psi.is_zero() {
                Element::ZERO
            } else {
                hash_merge([input.note.psi, input.secret_key])
            };

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

        Ok((blob_transaction, recipient_note))
    }
}
