use std::sync::Arc;

use anyhow::{Context, Result};
use client_sdk::rest_client::{NodeApiClient, NodeApiHttpClient};
use sdk::{BlobTransaction, ContractName, ProofTransaction, TxHash};

use crate::{
    keys::derive_key_material,
    tx::build_faucet_transaction,
    types::{KeyPairInfo, ZfruitAction},
};

/// High-level helper around the node REST API used by the faucet server.
///
/// The struct wraps a [`NodeApiHttpClient`] and exposes a few convenience helpers that
/// simplify the faucet workflow: generating deterministic keys from a name, building
/// the corresponding faucet blob transaction and optionally submitting it to the node.
pub struct FaucetApp {
    client: Arc<NodeApiHttpClient>,
    contract_name: ContractName,
}

impl FaucetApp {
    /// Create a new [`FaucetApp`] from a raw client.
    pub fn new(client: NodeApiHttpClient, contract_name: ContractName) -> Self {
        Self {
            client: Arc::new(client),
            contract_name,
        }
    }

    /// Build a [`FaucetApp`] by instantiating a [`NodeApiHttpClient`] from a base URL.
    pub fn from_url(node_url: impl Into<String>, contract_name: ContractName) -> Result<Self> {
        let client =
            NodeApiHttpClient::new(node_url.into()).context("creating node REST client")?;
        Ok(Self::new(client, contract_name))
    }

    /// Access the underlying REST client.
    pub fn client(&self) -> Arc<NodeApiHttpClient> {
        self.client.clone()
    }

    /// Name of the Noir contract the faucet operates on.
    pub fn contract_name(&self) -> &ContractName {
        &self.contract_name
    }

    /// Build the faucet blob transaction for `name`, optionally overriding `amount`.
    ///
    /// The function is pure (it only computes the key material and blob transaction) and
    /// does not submit anything to the node. The caller can decide whether to broadcast
    /// the transaction or just return the payload to a client.
    pub fn build_faucet_tx(
        &self,
        name: &str,
        amount: u64,
    ) -> Result<(KeyPairInfo, BlobTransaction)> {
        let key_material = derive_key_material(name)?;

        let tx = build_faucet_transaction(
            &self.contract_name,
            key_material.public_key.clone(),
            amount,
        );

        let key_pair = KeyPairInfo {
            private_key_hex: hex::encode(key_material.private_key),
            public_key_hex: hex::encode(key_material.public_key),
        };

        Ok((key_pair, tx))
    }

    /// Submit an already built blob transaction to the node.
    pub async fn submit_blob_transaction(&self, tx: BlobTransaction) -> Result<TxHash> {
        self.client
            .send_tx_blob(tx)
            .await
            .context("submitting blob transaction")
    }

    /// Submit a Noir proof transaction to the node.
    pub async fn submit_proof_transaction(&self, tx: ProofTransaction) -> Result<TxHash> {
        self.client
            .send_tx_proof(tx)
            .await
            .context("submitting proof transaction")
    }

    /// Convenience helper used by CLI / unit tests: build a faucet transaction for `name`,
    /// submit it to the node and return the resulting hash.
    pub async fn faucet_and_submit(&self, name: &str, amount: u64) -> Result<TxHash> {
        let (_keys, tx) = self.build_faucet_tx(name, amount)?;
        self.submit_blob_transaction(tx).await
    }

    /// Build the faucet blob action without creating a transaction.
    ///
    /// Useful when composing multi-blob transactions manually.
    pub fn build_action(&self, recipient_pubkey: Vec<u8>, amount: u64) -> ZfruitAction {
        ZfruitAction::Faucet {
            recipient_pubkey,
            amount,
        }
    }
}
