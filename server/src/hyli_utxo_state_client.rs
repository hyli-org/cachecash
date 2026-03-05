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
use hyli_utxo_state::{
    state::{ContractConfig, HyliUtxoState, HyliUtxoStateAction},
    zk::BorshableH256,
    HyliUtxoZkVmBatch, HyliUtxoZkVmState,
};
use sdk::{
    caller::ExecutionContext,
    utils::{as_hyli_output, parse_raw_calldata},
    Blob, BlobIndex, Calldata, Contract, ContractName, HyliOutput, RegisterContractAction,
    RunResult, StateCommitment,
};
use tracing::info;
use utoipa::openapi::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};

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
        note_keys: &[BorshableH256],
        nullified_keys: &[BorshableH256],
    ) -> Result<HyliUtxoZkVmState> {
        self.state
            .to_zkvm_state(self.config.clone(), note_keys, nullified_keys)
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

    fn update_from_blob(
        &mut self,
        calldata: &Calldata,
    ) -> Result<(Vec<BorshableH256>, Vec<BorshableH256>)> {
        let (_, state_blob) = calldata
            .blobs
            .iter()
            .find(|(index, _)| *index == calldata.index)
            .ok_or_else(|| anyhow!("state blob not found in calldata"))?;

        let action: HyliUtxoStateAction =
            BorshDeserialize::try_from_slice(&state_blob.data.0).map_err(|e| anyhow!(e))?;
        let (created, nullified) = Self::split_action(&action);
        info!(
            created_len = created.len(),
            nullified_len = nullified.len(),
            blob_index = %calldata.index.0,
            "applying hyli_utxo_state action"
        );
        self.apply_commitments(&nullified, &created)?;
        Ok((created, nullified))
    }

    /// The padding nullifier is poseidon2([0, 0], 2) - used by padding notes
    const PADDING_NULLIFIER: [u8; 32] = [
        0x0b, 0x63, 0xa5, 0x37, 0x87, 0x02, 0x1a, 0x4a, 0x96, 0x2a, 0x45, 0x2c, 0x29, 0x21, 0xb3,
        0x66, 0x3a, 0xff, 0x1f, 0xfd, 0x8d, 0x55, 0x10, 0x54, 0x0f, 0x8e, 0x65, 0x9e, 0x78, 0x29,
        0x56, 0xf1,
    ];

    fn split_action(action: &HyliUtxoStateAction) -> (Vec<BorshableH256>, Vec<BorshableH256>) {
        let created = action
            .iter()
            .take(2)
            .copied()
            .filter(|commitment| {
                let bytes: [u8; 32] = commitment.0.into();
                bytes != [0u8; 32]
            })
            .collect();

        let nullified = action
            .iter()
            .skip(2)
            .copied()
            .filter(|commitment| {
                let bytes: [u8; 32] = commitment.0.into();
                // Skip all-zeros AND the padding nullifier
                bytes != [0u8; 32] && bytes != Self::PADDING_NULLIFIER
            })
            .collect();

        (created, nullified)
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

    fn build_commitment_metadata(&self, blob: &Blob) -> Result<Vec<u8>> {
        let action: HyliUtxoStateAction =
            BorshDeserialize::try_from_slice(&blob.data.0).map_err(|e| anyhow!(e))?;

        let (created, nullified) = Self::split_action(&action);

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

        let Ok((_, execution_ctx)) = parse_raw_calldata::<HyliUtxoStateAction>(calldata)
            .map_err(|e| anyhow!("parsing calldata: {e}"))
        else {
            let _blob0: RegisterContractAction = borsh::from_slice(
                &calldata
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
    siblings_0: Vec<Vec<u8>>,
    siblings_1: Vec<Vec<u8>>,
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
        siblings_0: s0.iter().map(|a| a.to_vec()).collect(),
        siblings_1: s1.iter().map(|a| a.to_vec()).collect(),
    }))
}

impl ContractHandler for HyliUtxoStateExecutor {
    async fn api(store: ContractHandlerStore<Self>) -> (axum::Router<()>, OpenApi) {
        let (router, api) = OpenApiRouter::default()
            .routes(routes!(get_smt_witness))
            .split_for_parts();
        (router.with_state(store), api)
    }
}
