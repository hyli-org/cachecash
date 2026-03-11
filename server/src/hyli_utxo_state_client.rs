use acvm::AcirField;
use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{Query, State},
    Json,
};
use borsh::{BorshDeserialize, BorshSerialize};
use client_sdk::{
    contract_indexer::{ContractHandler, ContractHandlerStore},
    transaction_builder::TxExecutorHandler,
};
use hex::encode as hex_encode;
use hyli_modules::bus::BusMessage;
use hyli_utxo_state::{
    state::{
        parse_hyli_utxo_blob, ContractConfig, HyliUtxoState, HyliUtxoStateAction,
        SeparatedHyliUtxoBlob,
    },
    zk::BorshableH256,
    HyliUtxoZkVmBatch, HyliUtxoZkVmState,
};
use sdk::{
    caller::ExecutionContext, utils::as_hyli_output, BlobIndex, BlobTransaction, Calldata,
    Contract, ContractName, HyliOutput, RegisterContractAction, RunResult, StateCommitment,
    StructuredBlobData, TxContext,
};
use std::sync::Arc;
use tracing::info;
use utoipa::openapi::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};

/// Event emitted by [`HyliUtxoStateExecutor`] whenever a transaction is successfully settled.
/// Broadcast as `CSIBusEvent<HyliUtxoStateEvent>` on the message bus.
#[derive(Clone, Debug)]
pub struct HyliUtxoStateEvent {
    /// The new SMT notes root (big-endian bytes) after the transaction was applied.
    pub notes_root: [u8; 32],
}

impl BusMessage for HyliUtxoStateEvent {}

#[derive(Debug, BorshSerialize, BorshDeserialize)]
pub struct HyliUtxoStateExecutor {
    state: HyliUtxoState,
    config: ContractConfig,
}

impl Clone for HyliUtxoStateExecutor {
    fn clone(&self) -> Self {
        let encoded = borsh::to_vec(&self.state).expect("HyliUtxoState should serialize for clone");
        let state =
            borsh::from_slice(&encoded).expect("HyliUtxoState should deserialize for clone");
        let config = self.config.clone();
        Self { state, config }
    }
}

impl HyliUtxoStateExecutor {
    pub fn new(config: ContractConfig) -> Self {
        Self {
            state: HyliUtxoState::default(),
            config,
        }
    }

    pub fn utxo_state(&self) -> &HyliUtxoState {
        &self.state
    }

    pub fn zkvm_witness(
        &self,
        created_note_keys: &[BorshableH256],
        nullified_keys: &[BorshableH256],
    ) -> Result<HyliUtxoZkVmState> {
        self.state
            .to_zkvm_state(self.config.clone(), created_note_keys, nullified_keys)
            .map_err(|e| anyhow!(e))
    }

    fn apply_commitments(
        &mut self,
        nullified: &[BorshableH256],
        created: &[BorshableH256],
    ) -> Result<()> {
        // Debug: log the nullifiers being recorded
        for (i, n) in nullified.iter().enumerate() {
            let hex = hex_encode(n.0.as_slice());
            info!(index = i, nullifier = %hex, "recording nullifier");
        }
        for (i, c) in created.iter().enumerate() {
            let hex = hex_encode(c.0.as_slice());
            info!(index = i, commitment = %hex, "recording created commitment");
        }

        if !nullified.is_empty() {
            self.state
                .record_nullified(nullified)
                .map_err(|e| anyhow!(e))?;
        }
        if !created.is_empty() {
            self.state.record_created(created).map_err(|e| anyhow!(e))?;
        }
        self.state.update_roots();
        Ok(())
    }

    fn update_from_blob(&mut self, calldata: &Calldata) -> Result<SeparatedHyliUtxoBlob> {
        let (_, utxo_blob) = calldata
            .blobs
            .iter()
            .find(|(_, blob)| blob.contract_name == self.config.utxo_contract_name)
            .ok_or_else(|| anyhow!("state blob not found in calldata"))?;

        let (created, nullified) = parse_hyli_utxo_blob(&utxo_blob.data.0)
            .map_err(|e| anyhow!("parsing HyliUtxoBlob into commitments: {e}"))?;

        info!(
            created_len = created.len(),
            nullified_len = nullified.len(),
            blob_index = %calldata.index.0,
            "applying hyli_utxo_state action"
        );
        self.apply_commitments(&nullified, &created)?;
        Ok((created, nullified))
    }
}

impl TxExecutorHandler for HyliUtxoStateExecutor {
    type Contract = Self;

    fn construct_state(
        _contract_name: &ContractName,
        _contract: &Contract,
        metadata: &Option<Vec<u8>>,
    ) -> Result<Self> {
        let config = if let Some(bytes) = metadata {
            borsh::from_slice(bytes)
                .context("decoding ContractConfig from registration metadata")?
        } else {
            return Err(anyhow!(
                "Contract registration metadata is required to construct HyliUtxoStateExecutor"
            ));
        };

        Ok(HyliUtxoStateExecutor {
            state: HyliUtxoState::default(),
            config,
        })
    }

    fn build_commitment_metadata(&self, calldata: &Calldata) -> Result<Vec<u8>> {
        let (_, blob) = calldata
            .blobs
            .iter()
            .find(|(_, blob)| blob.contract_name == self.config.utxo_contract_name)
            .ok_or_else(|| {
                anyhow!(
                    "state blob for contract '{}' not found in calldata",
                    self.config.utxo_contract_name.0
                )
            })?;

        let (created, nullified) = parse_hyli_utxo_blob(&blob.data.0)
            .map_err(|e| anyhow!("parsing HyliUtxoBlob into commitments for metadata: {e}"))?;

        info!(
            created_len = created.len(),
            nullified_len = nullified.len(),
            blob_contract = %blob.contract_name.0,
            "built hyli_utxo_state commitment metadata"
        );

        let witness = self.zkvm_witness(&created, &nullified)?;
        let batch = HyliUtxoZkVmBatch::from_state(witness);
        borsh::to_vec(&batch).context("serializing HyliUtxoZkVmBatch")
    }

    fn merge_commitment_metadata(
        &self,
        initial: Vec<u8>,
        next: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        let mut initial_batch: HyliUtxoZkVmBatch =
            borsh::from_slice(&initial).map_err(|e| format!("decoding initial metadata: {e}"))?;
        let next_batch: HyliUtxoZkVmBatch =
            borsh::from_slice(&next).map_err(|e| format!("decoding next metadata: {e}"))?;

        initial_batch.extend_with(next_batch);

        borsh::to_vec(&initial_batch).map_err(|e| format!("serializing merged metadata: {e}"))
    }

    fn handle(&mut self, calldata: &Calldata) -> Result<HyliOutput> {
        let initial_commitment = self.get_state_commitment();

        let Some(state_blob) = calldata.blobs.get(&calldata.index) else {
            return Err(anyhow!("state blob not found in calldata"));
        };
        let execution_ctx =
            ExecutionContext::new(calldata.identity.clone(), state_blob.contract_name.clone());
        let parsed_state_action: Result<StructuredBlobData<HyliUtxoStateAction>, _> =
            state_blob.data.clone().try_into();

        let Ok(_) =
            parsed_state_action.map_err(|e| anyhow!("parsing structured state calldata: {e}"))
        else {
            let _blob0: RegisterContractAction = borsh::from_slice(
                calldata
                    .blobs
                    .get(&BlobIndex(0))
                    .map(|b| &b.data.0)
                    .ok_or_else(|| {
                        anyhow!("calldata did not match HyliUtxoStateAction, checking for RegisterContractAction, but blob index 0 not found in calldata for RegisterContractAction")
                    })?,
            )
            .map_err(|e| anyhow!("calldata did not match HyliUtxoStateAction, checking for RegisterContractAction but parsing first blob as RegisterContractAction failed: {e}"))?;

            return Ok(as_hyli_output(
                initial_commitment.clone(),
                initial_commitment,
                calldata,
                &mut Ok((
                    "Ignoring placeholder blob".as_bytes().to_vec(),
                    ExecutionContext::default(),
                    vec![],
                )),
            ));
        };

        let (created, nullified) = self.update_from_blob(calldata)?;

        let next_commitment = self.get_state_commitment();
        let initial_hex = hex_encode(&initial_commitment.0);
        let next_hex = hex_encode(&next_commitment.0);
        info!(
            created_len = created.len(),
            nullified_len = nullified.len(),
            initial_commitment = %initial_hex,
            next_commitment = %next_hex,
            "executed hyli_utxo_state action"
        );

        let mut result: RunResult = Ok((Vec::new(), execution_ctx, Vec::new()));

        Ok(as_hyli_output(
            initial_commitment,
            next_commitment,
            calldata,
            &mut result,
        ))
    }

    fn get_state_commitment(&self) -> StateCommitment {
        self.state.commitment()
    }
}

// ---- ContractHandler (SMT witness API) ----

#[derive(serde::Deserialize, utoipa::IntoParams)]
struct SmtWitnessQuery {
    commitment0: String,
    commitment1: Option<String>,
}

#[derive(serde::Serialize, utoipa::ToSchema)]
struct SmtWitnessResponse {
    notes_root: String,
    siblings_0: Vec<String>,
    siblings_1: Vec<String>,
}

fn parse_hex32(hex_str: &str) -> Result<BorshableH256, String> {
    let normalized = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let bytes = hex::decode(normalized).map_err(|e| format!("invalid hex: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!("expected 32 bytes, got {}", bytes.len()));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(BorshableH256::from(arr))
}

#[utoipa::path(
    get,
    path = "/smt-witness",
    params(SmtWitnessQuery),
    responses(
        (status = 200, description = "SMT witness for the given commitments", body = SmtWitnessResponse),
    )
)]
async fn get_smt_witness(
    Query(params): Query<SmtWitnessQuery>,
    State(store): State<ContractHandlerStore<HyliUtxoStateExecutor>>,
) -> Result<Json<SmtWitnessResponse>, (axum::http::StatusCode, String)> {
    let store = store.read().await;
    let executor = store.state.as_ref().ok_or_else(|| {
        (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "State not yet initialized".to_string(),
        )
    })?;

    let c0 =
        parse_hex32(&params.commitment0).map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e))?;
    let c1 = match params.commitment1.as_deref() {
        Some(s) => parse_hex32(s).map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e))?,
        None => BorshableH256::from([0u8; 32]),
    };

    let notes_root = executor.utxo_state().notes_root();
    let (s0, s1) = executor.utxo_state().build_smt_witnesses(c0, c1);

    Ok(Json(SmtWitnessResponse {
        notes_root: hex::encode(notes_root.as_ref()),
        siblings_0: s0
            .iter()
            .map(|f| format!("0x{}", hex::encode(f.to_be_bytes())))
            .collect(),
        siblings_1: s1
            .iter()
            .map(|f| format!("0x{}", hex::encode(f.to_be_bytes())))
            .collect(),
    }))
}

impl ContractHandler<HyliUtxoStateEvent> for HyliUtxoStateExecutor {
    fn handle_transaction_success(
        &mut self,
        tx: &BlobTransaction,
        index: BlobIndex,
        _tx_context: Arc<TxContext>,
    ) -> Result<Option<HyliUtxoStateEvent>> {
        // Build calldata and apply the blob (update_from_blob → apply_commitments → update_roots)
        let calldata = sdk::Calldata {
            identity: tx.identity.clone(),
            index,
            blobs: tx.blobs.clone().into(),
            tx_blob_count: tx.blobs.len(),
            tx_hash: sdk::Hashed::hashed(tx),
            tx_ctx: None,
            private_input: vec![],
        };
        if let Err(e) = self.handle(&calldata) {
            tracing::error!("Failed to handle blob {index} for hyli_utxo_state: {e}");
            return Ok(None);
        }
        let notes_root: [u8; 32] = self.state.notes_root().into();
        Ok(Some(HyliUtxoStateEvent { notes_root }))
    }

    async fn api(store: ContractHandlerStore<Self>) -> (axum::Router<()>, OpenApi) {
        let (router, api) = OpenApiRouter::default()
            .routes(routes!(get_smt_witness))
            .split_for_parts();
        (router.with_state(store), api)
    }
}
