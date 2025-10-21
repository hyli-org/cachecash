use anyhow::{anyhow, Context, Result};
use client_sdk::rest_client::{NodeApiClient, NodeApiHttpClient};
use element::Element;
use hash::hash_merge;
use hyli_modules::{
    bus::{BusMessage, SharedMessageBus},
    module_bus_client, module_handle_messages,
    modules::Module,
};
use sdk::{Blob, BlobData, BlobTransaction, Hashed, Identity, TxHash};
use tracing::{error, info};
use zk_primitives::{Note, Utxo, HYLI_BLOB_HASH_BYTE_LENGTH, HYLI_BLOB_LENGTH_BYTES};

use crate::{
    keys::KeyMaterial,
    tx::{FAUCET_IDENTITY_PREFIX, HYLI_UTXO_CONTRACT_NAME},
};

pub const FAUCET_MINT_AMOUNT: u64 = 10;
const MINT_AMOUNT: u64 = FAUCET_MINT_AMOUNT;

#[derive(Clone, Debug)]
pub struct FaucetMintRequest {
    pub key_material: KeyMaterial,
    pub amount: u64,
}

impl BusMessage for FaucetMintRequest {}

module_bus_client! {
pub struct FaucetBusClient {
    receiver(FaucetMintRequest),
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
            listen<FaucetMintRequest> request => {
                if let Err(err) = self.process_request(request).await {
                    error!(error = %err, "Faucet mint request failed");
                }
            }
        };

        Ok(())
    }
}

impl FaucetApp {
    async fn process_request(&mut self, request: FaucetMintRequest) -> Result<()> {
        if request.amount != MINT_AMOUNT {
            return Err(anyhow!(
                "unsupported faucet amount {}; expected {MINT_AMOUNT}",
                request.amount
            ));
        }

        let tx_hash = self
            .build_and_send_transaction(&request.key_material, request.amount)
            .await
            .context("sending faucet transaction")?;

        info!(%tx_hash, "Submitted hyli_utxo faucet transaction");
        Ok(())
    }

    async fn build_and_send_transaction(
        &mut self,
        key_material: &KeyMaterial,
        amount: u64,
    ) -> Result<TxHash> {
        let blob_bytes = self.build_blob(key_material, amount)?;

        let contract_name = HYLI_UTXO_CONTRACT_NAME.to_string();
        let identity = Identity(format!("{}@{}", FAUCET_IDENTITY_PREFIX, contract_name));
        let blob_transaction = BlobTransaction::new(
            identity,
            vec![Blob {
                contract_name: contract_name.into(),
                data: BlobData(blob_bytes),
            }],
        );

        let tx_hash = blob_transaction.hashed();

        self.client
            .send_tx_blob(blob_transaction)
            .await
            .context("dispatching blob transaction")?;

        Ok(tx_hash)
    }

    fn build_blob(&mut self, key_material: &KeyMaterial, amount: u64) -> Result<Vec<u8>> {
        let private_key = Element::from_be_bytes(key_material.private_key);
        let minted_value = Element::new(amount);

        let recipient_note = Note::new_from_ephemeral_private_key(private_key, minted_value);
        let utxo = Utxo::new_mint([recipient_note, Note::padding_note()]);

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
        }

        Ok(blob_bytes)
    }
}
