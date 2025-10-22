use anyhow::{anyhow, bail, Context, Result};
use client_sdk::rest_client::{NodeApiClient, NodeApiHttpClient};
use sdk::{
    api::APIRegisterContract, ContractName, ProgramId, StateCommitment, TxHash, Verifier,
    ZkContract,
};
use tracing::info;

use contracts::HYLI_UTXO_STATE_VK;
use hyli_utxo_state::HyliUtxoZkVmState;

use crate::tx::HYLI_UTXO_CONTRACT_NAME;

pub const HYLI_UTXO_STATE_CONTRACT_NAME: &str = "hyli-utxo-state";

const HYLI_UTXO_NOIR_VK: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../fixtures/keys/hyli_utxo_key"
));

/// Metadata required to deploy (or validate) a Noir UTXO contract.
#[derive(Clone, Debug)]
pub struct ContractDeployment {
    pub contract_name: ContractName,
    pub program_id: ProgramId,
    pub state_commitment: StateCommitment,
    pub timeout_window: Option<u64>,
}

impl ContractDeployment {
    /// Build the REST payload used to register the contract on the node.
    pub fn to_register_payload(&self, verifier: Verifier) -> APIRegisterContract {
        APIRegisterContract {
            verifier,
            program_id: self.program_id.clone(),
            state_commitment: self.state_commitment.clone(),
            contract_name: self.contract_name.clone(),
            timeout_window: self.timeout_window,
            constructor_metadata: None,
        }
    }
}

pub fn hyli_utxo_noir_deployment() -> ContractDeployment {
    ContractDeployment {
        contract_name: ContractName(HYLI_UTXO_CONTRACT_NAME.to_string()),
        program_id: ProgramId(HYLI_UTXO_NOIR_VK.to_vec()),
        state_commitment: StateCommitment(vec![0u8; 4]),
        timeout_window: None,
    }
}

pub fn hyli_utxo_state_deployment() -> ContractDeployment {
    let initial_state = HyliUtxoZkVmState::default();
    ContractDeployment {
        contract_name: ContractName(HYLI_UTXO_STATE_CONTRACT_NAME.to_string()),
        program_id: ProgramId(HYLI_UTXO_STATE_VK.to_vec()),
        state_commitment: initial_state.commit(),
        timeout_window: None,
    }
}

/// Ensure that the Noir contract is registered on the node.
///
/// - If the contract already exists, the function validates that the on-chain program id
///   matches the one provided locally.
/// - Otherwise, a registration request is submitted and the function waits until the
///   contract becomes queryable.
pub async fn ensure_contract_registered(
    client: &NodeApiHttpClient,
    deployment: &ContractDeployment,
    verifier: Verifier,
) -> Result<()> {
    match client.get_contract(deployment.contract_name.clone()).await {
        Ok(existing) => {
            if existing.program_id != deployment.program_id {
                let node_pid = hex::encode(existing.program_id.0);
                let local_pid = hex::encode(&deployment.program_id.0);
                bail!(
                    "program id mismatch for contract {} (node: {}, local: {})",
                    deployment.contract_name.0,
                    node_pid,
                    local_pid,
                );
            }

            info!(
                contract = %deployment.contract_name.0,
                "Contract already registered on the node"
            );
            Ok(())
        }
        Err(_) => {
            info!(
                contract = %deployment.contract_name.0,
                "Registering Noir contract on the node"
            );

            let payload = deployment.to_register_payload(verifier);
            let tx_hash = client
                .register_contract(payload)
                .await
                .context("registering contract with the node")?;
            wait_for_contract(client, &deployment.contract_name, Some(tx_hash)).await
        }
    }
}

/// Poll the node until the contract information becomes available.
pub async fn wait_for_contract(
    client: &NodeApiHttpClient,
    contract_name: &ContractName,
    _submitted_tx: Option<TxHash>,
) -> Result<()> {
    let mut attempts = 0_u32;
    loop {
        attempts += 1;
        match client.get_contract(contract_name.clone()).await {
            Ok(_) => {
                info!(
                    contract = %contract_name.0,
                    attempts,
                    "Contract registration confirmed"
                );
                return Ok(());
            }
            Err(err) if attempts < 60 => {
                tracing::debug!(
                    contract = %contract_name.0,
                    attempts,
                    error = %err,
                    "Waiting for contract to be discoverable"
                );
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Err(err) => {
                return Err(anyhow!(
                    "unable to confirm contract registration after {attempts} tries: {err}"
                ));
            }
        }
    }
}
