use std::sync::Arc;

use crate::{
    app::{
        build_note, FaucetDepositCommand, FaucetMintCommand, TransferCommand,
        TransferWithProofCommand, FAUCET_MINT_AMOUNT,
    },
    init::{HYLI_SMT_INCL_PROOF_VK, HYLI_UTXO_NOIR_VK},
    metrics::FaucetMetrics,
    note_store::{AddressRegistry, NoteStore},
    smt_incl_prover::HyliSmtInclNoirProver,
    types::{
        BlobHashResponse, BlobInfo, CreateBlobRequest, CreateBlobResponse, DepositRequest,
        EncryptedNoteRecord, FaucetRequest, FaucetResponse, FinalizeTransferRequest,
        FinalizeTransferResponse, GetNotesQuery, GetNotesResponse, InputNoteData,
        RegisterAddressRequest, RegisterAddressResponse, ResolveAddressResponse,
        ServerConfigResponse, SubmitProofRequest, TokenTransferRequest, TransferResponse,
        UploadNoteRequest, UploadNoteResponse,
    },
};
use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use client_sdk::rest_client::{NodeApiClient, NodeApiHttpClient};
use hyli_modules::{
    bus::{BusClientSender, SharedMessageBus},
    module_bus_client, module_handle_messages,
    modules::{BuildApiContextInner, Module},
};
use hyli_smt_token::SmtTokenAction;
use hyli_utxo_state::state::HYLI_UTXO_STATE_ACTION;
use sdk::{
    Blob, BlobData, BlobIndex, BlobTransaction, ContractAction, ContractName, Hashed, Identity,
    ProgramId, ProofData, ProofTransaction, StructuredBlobData, TxHash, Verifier,
};
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};
use zk_primitives::{InputNote, ToBytes, HYLI_SMT_INCL_BLOB_LENGTH_BYTES};

pub struct ApiModule {
    bus: ApiModuleBusClient,
}

pub struct ApiModuleCtx {
    pub api: Arc<BuildApiContextInner>,
    pub default_amount: u64,
    pub contract_name: ContractName,
    pub metrics: FaucetMetrics,
    pub note_store: Arc<NoteStore>,
    pub address_registry: Arc<AddressRegistry>,
    pub max_note_payload_size: usize,
    pub client: NodeApiHttpClient,
    pub utxo_contract_name: String,
    pub utxo_state_contract_name: String,
    pub smt_incl_proof_contract_name: String,
}

#[derive(Clone)]
struct RouterCtx {
    default_amount: u64,
    bus: ApiModuleBusClient,
    metrics: FaucetMetrics,
    note_store: Arc<NoteStore>,
    address_registry: Arc<AddressRegistry>,
    max_note_payload_size: usize,
    client: NodeApiHttpClient,
    utxo_contract_name: String,
    utxo_state_contract_name: String,
    smt_incl_proof_contract_name: String,
}

module_bus_client! {
    #[derive(Debug)]
pub struct ApiModuleBusClient {
        sender(FaucetMintCommand),
        sender(FaucetDepositCommand),
        sender(TransferCommand),
        sender(TransferWithProofCommand),
    }
}

impl Module for ApiModule {
    type Context = Arc<ApiModuleCtx>;

    async fn build(bus: SharedMessageBus, ctx: Self::Context) -> Result<Self> {
        let module_bus = ApiModuleBusClient::new_from_bus(bus.new_handle()).await;

        let router_ctx = RouterCtx {
            default_amount: ctx.default_amount,
            bus: module_bus.clone(),
            metrics: ctx.metrics.clone(),
            note_store: ctx.note_store.clone(),
            address_registry: ctx.address_registry.clone(),
            max_note_payload_size: ctx.max_note_payload_size,
            client: ctx.client.clone(),
            utxo_contract_name: ctx.utxo_contract_name.clone(),
            utxo_state_contract_name: ctx.utxo_state_contract_name.clone(),
            smt_incl_proof_contract_name: ctx.smt_incl_proof_contract_name.clone(),
        };

        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET, Method::POST, Method::DELETE])
            .allow_headers(Any);

        let router = Router::new()
            .route("/_health", get(health))
            .route("/api/config", get(get_config))
            .route("/api/faucet", post(faucet))
            .route("/api/deposit", post(deposit))
            // Two-step transfer endpoints (client-side proving with real tx_hash)
            .route("/api/blob/create", post(create_blob))
            .route("/api/proof/submit", post(submit_proof))
            // Atomic transfer: compute tx_hash before proving, then submit all at once
            .route("/api/blob/hash", post(hash_blob))
            .route("/api/transfer/finalize", post(finalize_transfer))
            // Encrypted notes endpoints
            .route("/api/notes", post(upload_note))
            .route("/api/notes/{recipient_tag}", get(get_notes))
            .route("/api/notes/{recipient_tag}/{note_id}", delete(delete_note))
            // Address registry endpoints
            .route("/api/address/register", post(register_address))
            .route("/api/address/resolve/{username}", get(resolve_address))
            .with_state(router_ctx)
            .layer(cors);

        if let Ok(mut guard) = ctx.api.router.lock() {
            let merged = guard.take().unwrap_or_else(Router::new).merge(router);
            guard.replace(merged);
        }

        Ok(Self { bus: module_bus })
    }

    async fn run(&mut self) -> Result<()> {
        module_handle_messages! {
            on_self self,
        };
        Ok(())
    }
}

async fn health() -> &'static str {
    "OK"
}

async fn get_config(State(state): State<RouterCtx>) -> Json<ServerConfigResponse> {
    Json(ServerConfigResponse {
        contract_name: state.utxo_contract_name.clone(),
        utxo_state_contract_name: state.utxo_state_contract_name.clone(),
        smt_incl_proof_contract_name: state.smt_incl_proof_contract_name.clone(),
    })
}

async fn faucet(
    State(state): State<RouterCtx>,
    Json(request): Json<FaucetRequest>,
) -> Result<Json<FaucetResponse>, ApiError> {
    let RouterCtx {
        default_amount,
        mut bus,
        metrics,
        ..
    } = state;

    let default_amount = if default_amount == 0 {
        FAUCET_MINT_AMOUNT
    } else {
        default_amount
    };
    let amount = request.amount.unwrap_or(default_amount);
    if amount == 0 {
        metrics.record_failure("invalid_amount");
        return Err(ApiError::bad_request("amount must be greater than zero"));
    }

    let pubkey_hex = request.pubkey_hex.trim();
    if pubkey_hex.is_empty() {
        metrics.record_failure("missing_pubkey");
        return Err(ApiError::bad_request("pubkey_hex must not be empty"));
    }

    let normalized_pubkey = pubkey_hex.strip_prefix("0x").unwrap_or(pubkey_hex);
    let pubkey_bytes = match hex::decode(normalized_pubkey) {
        Ok(bytes) => bytes,
        Err(err) => {
            metrics.record_failure("invalid_pubkey_hex");
            return Err(ApiError::bad_request(format!("invalid pubkey_hex: {err}")));
        }
    };

    if pubkey_bytes.len() != 32 {
        metrics.record_failure("invalid_pubkey_length");
        return Err(ApiError::bad_request("pubkey_hex must decode to 32 bytes"));
    }

    let mut address_bytes = [0u8; 32];
    address_bytes.copy_from_slice(&pubkey_bytes);
    let recipient_address = element::Element::from_be_bytes(address_bytes);

    let note = build_note(recipient_address, amount);

    bus.send(FaucetMintCommand {
        recipient_pubkey: pubkey_bytes,
        amount,
        note: note.clone(),
    })
    .map_err(|err| {
        metrics.record_failure("bus_send_failed");
        ApiError::internal(err.to_string())
    })?;

    let response = FaucetResponse { note };
    metrics.record_success(amount);

    Ok(Json(response))
}

async fn deposit(
    State(state): State<RouterCtx>,
    Json(request): Json<DepositRequest>,
) -> Result<Json<FaucetResponse>, ApiError> {
    let RouterCtx {
        default_amount,
        mut bus,
        metrics,
        ..
    } = state;

    let default_amount = if default_amount == 0 {
        FAUCET_MINT_AMOUNT
    } else {
        default_amount
    };
    let amount = request.amount.unwrap_or(default_amount);
    if amount == 0 {
        metrics.record_failure("invalid_amount");
        return Err(ApiError::bad_request("amount must be greater than zero"));
    }

    let pubkey_hex = request.pubkey_hex.trim();
    if pubkey_hex.is_empty() {
        metrics.record_failure("missing_pubkey");
        return Err(ApiError::bad_request("pubkey_hex must not be empty"));
    }

    let normalized_pubkey = pubkey_hex.strip_prefix("0x").unwrap_or(pubkey_hex);
    let pubkey_bytes = match hex::decode(normalized_pubkey) {
        Ok(bytes) => bytes,
        Err(err) => {
            metrics.record_failure("invalid_pubkey_hex");
            return Err(ApiError::bad_request(format!("invalid pubkey_hex: {err}")));
        }
    };

    if pubkey_bytes.len() != 32 {
        metrics.record_failure("invalid_pubkey_length");
        return Err(ApiError::bad_request("pubkey_hex must decode to 32 bytes"));
    }

    let token_contract = request
        .token_contract
        .unwrap_or_else(|| "oranj".to_string())
        .trim()
        .to_string();
    if token_contract.is_empty() {
        metrics.record_failure("missing_token_contract");
        return Err(ApiError::bad_request("token_contract must not be empty"));
    }

    let mut address_bytes = [0u8; 32];
    address_bytes.copy_from_slice(&pubkey_bytes);
    let recipient_address = element::Element::from_be_bytes(address_bytes);

    let note = build_note(recipient_address, amount);

    bus.send(FaucetDepositCommand {
        recipient_pubkey: pubkey_bytes,
        amount,
        note: note.clone(),
        token_contract,
        wallet_account: request.wallet_account,
        secp256k1_blob: request.secp256k1_blob,
        wallet_blob: request.wallet_blob,
    })
    .map_err(|err| {
        metrics.record_failure("bus_send_failed");
        ApiError::internal(err.to_string())
    })?;

    let response = FaucetResponse { note };
    metrics.record_success(amount);

    Ok(Json(response))
}

/// Create a blob transaction and return the tx_hash (legacy two-step endpoint).
/// Prefer /api/blob/hash + /api/transfer/finalize for atomic submission.
async fn create_blob(
    State(state): State<RouterCtx>,
    Json(request): Json<CreateBlobRequest>,
) -> Result<Json<CreateBlobResponse>, ApiError> {
    if request.blob_data.len() != 128 {
        return Err(ApiError::bad_request(format!(
            "blob_data must be exactly 128 bytes, got {}",
            request.blob_data.len()
        )));
    }

    let built = build_blob_transaction(&state, &request)?;

    let tx_hash = state
        .client
        .send_tx_blob(built.transaction)
        .await
        .map_err(|e| ApiError::internal(format!("failed to send blob tx: {}", e)))?;

    tracing::info!(%tx_hash, "Submitted blob transaction (create_blob)");

    let blobs = vec![
        BlobInfo {
            contract_name: state.utxo_state_contract_name.clone(),
            data: built.state_blob_hex,
        },
        BlobInfo {
            contract_name: state.utxo_contract_name.clone(),
            data: built.hyli_utxo_hex,
        },
        BlobInfo {
            contract_name: state.smt_incl_proof_contract_name.clone(),
            data: built.smt_hex,
        },
    ];

    Ok(Json(CreateBlobResponse {
        tx_hash: hex::encode(&tx_hash.0),
        blobs,
    }))
}

/// Submit a proof for an existing blob transaction (step 2 of two-step transfer)
async fn submit_proof(
    State(state): State<RouterCtx>,
    Json(request): Json<SubmitProofRequest>,
) -> Result<Json<TransferResponse>, ApiError> {
    // Validate tx_hash
    if request.tx_hash.0.is_empty() {
        return Err(ApiError::bad_request("tx_hash must not be empty"));
    }

    // ---- hyli_utxo proof ----
    let proof_bytes = base64_decode(&request.proof)
        .map_err(|e| ApiError::bad_request(format!("invalid base64 proof: {}", e)))?;

    let public_inputs_bytes: Vec<u8> = request
        .public_inputs
        .iter()
        .flat_map(|hex_str| {
            let normalized = hex_str.strip_prefix("0x").unwrap_or(hex_str);
            hex::decode(normalized).unwrap_or_else(|_| vec![0u8; 32])
        })
        .collect();

    let mut proof_with_inputs = public_inputs_bytes;
    proof_with_inputs.extend_from_slice(&proof_bytes);

    let utxo_proof_tx = ProofTransaction {
        contract_name: ContractName(state.utxo_contract_name.clone()),
        program_id: ProgramId(HYLI_UTXO_NOIR_VK.to_vec()),
        verifier: Verifier(sdk::verifiers::NOIR.to_string()),
        proof: ProofData(proof_with_inputs),
    };

    state
        .client
        .send_tx_proof(utxo_proof_tx)
        .await
        .map_err(|e| ApiError::internal(format!("failed to send utxo proof tx: {}", e)))?;

    // ---- hyli_smt_incl_proof proof ----
    let smt_proof_bytes = base64_decode(&request.smt_proof)
        .map_err(|e| ApiError::bad_request(format!("invalid base64 smt_proof: {}", e)))?;

    let smt_public_inputs_bytes: Vec<u8> = request
        .smt_public_inputs
        .iter()
        .flat_map(|hex_str| {
            let normalized = hex_str.strip_prefix("0x").unwrap_or(hex_str);
            hex::decode(normalized).unwrap_or_else(|_| vec![0u8; 32])
        })
        .collect();

    let mut smt_proof_with_inputs = smt_public_inputs_bytes;
    smt_proof_with_inputs.extend_from_slice(&smt_proof_bytes);

    let smt_proof_tx = ProofTransaction {
        contract_name: ContractName(state.smt_incl_proof_contract_name.clone()),
        program_id: ProgramId(HYLI_SMT_INCL_PROOF_VK.to_vec()),
        verifier: Verifier(sdk::verifiers::NOIR.to_string()),
        proof: ProofData(smt_proof_with_inputs),
    };

    state
        .client
        .send_tx_proof(smt_proof_tx)
        .await
        .map_err(|e| ApiError::internal(format!("failed to send smt proof tx: {}", e)))?;

    tracing::info!(tx_hash = %request.tx_hash, "Submitted proof transactions (step 2 of two-step transfer)");

    Ok(Json(TransferResponse {
        tx_hash: request.tx_hash,
        change_note: None,
    }))
}

// ---- Shared blob-building helper ----

struct BuiltBlob {
    transaction: BlobTransaction,
    state_blob_hex: String,
    hyli_utxo_hex: String,
    smt_hex: String,
}

fn withdraw_topology(
    utxo_state_contract_name: &str,
    token_transfer: Option<&TokenTransferRequest>,
) -> (Vec<BlobIndex>, Option<BlobIndex>) {
    let is_withdraw = token_transfer
        .is_some_and(|token_transfer| token_transfer.sender == utxo_state_contract_name);

    let mut state_callees = vec![BlobIndex(2)];
    let token_caller = if is_withdraw {
        state_callees.push(BlobIndex(3));
        Some(BlobIndex(0))
    } else {
        None
    };

    (state_callees, token_caller)
}

fn build_blob_transaction(
    state: &RouterCtx,
    request: &CreateBlobRequest,
) -> Result<BuiltBlob, ApiError> {
    let mut nullifier_0 = [0u8; 32];
    let mut nullifier_1 = [0u8; 32];
    nullifier_0.copy_from_slice(&request.blob_data[64..96]);
    nullifier_1.copy_from_slice(&request.blob_data[96..128]);

    // Resolve smt_blob_data: client-provided or computed from input_notes + notes_root
    let raw_smt_blob_data = match &request.smt_blob_data {
        Some(data) => {
            if data.len() != 96 {
                return Err(ApiError::bad_request(format!(
                    "smt_blob_data must be exactly 96 bytes, got {}",
                    data.len()
                )));
            }
            data.clone()
        }
        None => {
            let notes = request.input_notes.as_ref().ok_or_else(|| {
                ApiError::bad_request(
                    "input_notes is required when smt_blob_data is not provided".to_string(),
                )
            })?;
            let root_hex = request.notes_root.as_ref().ok_or_else(|| {
                ApiError::bad_request(
                    "notes_root is required when smt_blob_data is not provided".to_string(),
                )
            })?;
            compute_smt_blob_data(notes, root_hex)?
        }
    };

    let contract_name = state.utxo_contract_name.clone();
    let identity = Identity(format!("transfer@{}", contract_name));
    let hyli_utxo_data = BlobData(request.blob_data.clone());
    let (state_callees, token_caller) = withdraw_topology(
        &state.utxo_state_contract_name,
        request.token_transfer.as_ref(),
    );
    let state_blob_data = BlobData::from(StructuredBlobData {
        caller: None,
        callees: Some(state_callees),
        parameters: HYLI_UTXO_STATE_ACTION,
    });
    let smt_blob_data = BlobData::from(StructuredBlobData {
        caller: Some(BlobIndex(0)),
        callees: None,
        parameters: raw_smt_blob_data,
    });

    let state_blob = Blob {
        contract_name: ContractName(state.utxo_state_contract_name.clone()),
        data: state_blob_data.clone(),
    };
    let hyli_utxo_blob = Blob {
        contract_name: ContractName(contract_name.clone()),
        data: hyli_utxo_data.clone(),
    };
    let smt_incl_proof_blob = Blob {
        contract_name: ContractName(state.smt_incl_proof_contract_name.clone()),
        data: smt_blob_data.clone(),
    };

    let mut blobs = vec![state_blob, hyli_utxo_blob, smt_incl_proof_blob];
    if let Some(TokenTransferRequest {
        token_contract,
        sender,
        recipient,
        amount,
    }) = &request.token_transfer
    {
        let token_action = SmtTokenAction::Transfer {
            sender: sender.clone().into(),
            recipient: recipient.clone().into(),
            amount: *amount as u128,
        };
        blobs.push(token_action.as_blob(token_contract.clone().into(), token_caller, None));
    }

    let transaction = BlobTransaction::new(identity, blobs);

    Ok(BuiltBlob {
        transaction,
        state_blob_hex: hex::encode(&state_blob_data.0),
        hyli_utxo_hex: hex::encode(&hyli_utxo_data.0),
        smt_hex: hex::encode(&smt_blob_data.0),
    })
}

/// Compute the tx_hash for blob data without submitting to the chain.
/// Client uses this to generate proofs with the real tx_hash, then calls /api/transfer/finalize.
async fn hash_blob(
    State(state): State<RouterCtx>,
    Json(request): Json<CreateBlobRequest>,
) -> Result<Json<BlobHashResponse>, ApiError> {
    if request.blob_data.len() != 128 {
        return Err(ApiError::bad_request(format!(
            "blob_data must be exactly 128 bytes, got {}",
            request.blob_data.len()
        )));
    }

    let built = build_blob_transaction(&state, &request)?;
    let tx_hash = built.transaction.hashed();

    Ok(Json(BlobHashResponse { tx_hash }))
}

/// Submit blob transaction + both proofs atomically.
/// Client must have called /api/blob/hash first to get tx_hash for proof generation.
///
/// The SMT inclusion proof can either be supplied by the client (`smt_proof` +
/// `smt_public_inputs`) or generated server-side when those fields are omitted.
/// For server-side generation the client must provide `input_notes`, `siblings_0`,
/// and `siblings_1`.
async fn finalize_transfer(
    State(state): State<RouterCtx>,
    Json(request): Json<FinalizeTransferRequest>,
) -> Result<Json<FinalizeTransferResponse>, ApiError> {
    // Destructure upfront to avoid partial-move issues
    let FinalizeTransferRequest {
        blob_data,
        smt_blob_data,
        output_notes,
        token_transfer,
        proof,
        public_inputs,
        smt_proof,
        smt_public_inputs,
        input_notes,
        siblings_0,
        siblings_1,
        notes_root,
    } = request;

    if blob_data.len() != 128 {
        return Err(ApiError::bad_request(format!(
            "blob_data must be exactly 128 bytes, got {}",
            blob_data.len()
        )));
    }

    let blob_request = CreateBlobRequest {
        blob_data,
        smt_blob_data,
        output_notes,
        token_transfer,
        input_notes: input_notes.clone(),
        notes_root: notes_root.clone(),
    };
    let built = build_blob_transaction(&state, &blob_request)?;
    let tx_hash = built.transaction.hashed();

    // Extract SMT blob data before the transaction is moved (BlobTransaction is not Clone)
    let smt_blob_info = if smt_proof.is_none() {
        let (smt_blob_index, smt_blob) = built
            .transaction
            .blobs
            .iter()
            .enumerate()
            .find(|(_, blob)| blob.contract_name.0 == state.smt_incl_proof_contract_name)
            .ok_or_else(|| {
                ApiError::internal("hyli_smt_incl_proof blob not found in transaction".to_string())
            })?;

        if smt_blob.data.0.len() != HYLI_SMT_INCL_BLOB_LENGTH_BYTES {
            return Err(ApiError::internal(format!(
                "hyli_smt_incl_proof blob is {} bytes, expected {}",
                smt_blob.data.0.len(),
                HYLI_SMT_INCL_BLOB_LENGTH_BYTES
            )));
        }

        let mut blob_payload = [0u8; HYLI_SMT_INCL_BLOB_LENGTH_BYTES];
        blob_payload.copy_from_slice(&smt_blob.data.0);

        Some((
            blob_payload,
            built.transaction.identity.clone(),
            built.transaction.blobs.len() as u32,
            smt_blob_index as u32,
        ))
    } else {
        None
    };

    // Submit blob transaction
    state
        .client
        .send_tx_blob(built.transaction)
        .await
        .map_err(|e| ApiError::internal(format!("failed to send blob tx: {}", e)))?;

    tracing::info!(%tx_hash, "Submitted blob transaction (finalize_transfer)");

    // ---- hyli_utxo proof ----
    let proof_bytes = base64_decode(&proof)
        .map_err(|e| ApiError::bad_request(format!("invalid base64 proof: {}", e)))?;

    let public_inputs_bytes: Vec<u8> = public_inputs
        .iter()
        .flat_map(|hex_str| {
            let normalized = hex_str.strip_prefix("0x").unwrap_or(hex_str);
            hex::decode(normalized).unwrap_or_else(|_| vec![0u8; 32])
        })
        .collect();

    let mut proof_with_inputs = public_inputs_bytes;
    proof_with_inputs.extend_from_slice(&proof_bytes);

    state
        .client
        .send_tx_proof(ProofTransaction {
            contract_name: ContractName(state.utxo_contract_name.clone()),
            program_id: ProgramId(HYLI_UTXO_NOIR_VK.to_vec()),
            verifier: Verifier(sdk::verifiers::NOIR.to_string()),
            proof: ProofData(proof_with_inputs),
        })
        .await
        .map_err(|e| ApiError::internal(format!("failed to send utxo proof tx: {}", e)))?;

    // ---- hyli_smt_incl_proof proof ----
    let smt_proof_with_inputs = match smt_proof {
        Some(client_proof) => {
            // Client-provided proof
            let smt_proof_bytes = base64_decode(&client_proof)
                .map_err(|e| ApiError::bad_request(format!("invalid base64 smt_proof: {}", e)))?;
            let smt_pubs = smt_public_inputs.ok_or_else(|| {
                ApiError::bad_request(
                    "smt_public_inputs is required when smt_proof is provided".to_string(),
                )
            })?;
            let smt_public_inputs_bytes: Vec<u8> = smt_pubs
                .iter()
                .flat_map(|hex_str| {
                    let normalized = hex_str.strip_prefix("0x").unwrap_or(hex_str);
                    hex::decode(normalized).unwrap_or_else(|_| vec![0u8; 32])
                })
                .collect();
            let mut combined = smt_public_inputs_bytes;
            combined.extend_from_slice(&smt_proof_bytes);
            combined
        }
        None => {
            // Server-side proof generation
            let (blob_payload, identity, tx_blob_count, smt_blob_index) =
                smt_blob_info.expect("smt_blob_info must be Some when smt_proof is None");
            generate_smt_proof(
                &state,
                &tx_hash,
                blob_payload,
                identity,
                tx_blob_count,
                smt_blob_index,
                input_notes,
                siblings_0,
                siblings_1,
            )
            .await?
        }
    };

    state
        .client
        .send_tx_proof(ProofTransaction {
            contract_name: ContractName(state.smt_incl_proof_contract_name.clone()),
            program_id: ProgramId(HYLI_SMT_INCL_PROOF_VK.to_vec()),
            verifier: Verifier(sdk::verifiers::NOIR.to_string()),
            proof: ProofData(smt_proof_with_inputs),
        })
        .await
        .map_err(|e| ApiError::internal(format!("failed to send smt proof tx: {}", e)))?;

    tracing::info!(%tx_hash, "Submitted proof transactions (finalize_transfer)");

    Ok(Json(FinalizeTransferResponse { tx_hash }))
}

/// Compute the 96-byte `smt_blob_data` from input notes and notes root.
///
/// Layout: [nullifier_0 (32B)][nullifier_1 (32B)][notes_root (32B)]
/// Nullifiers are computed as `hash_merge([psi, secret_key])`.
fn compute_smt_blob_data(
    input_notes: &[InputNoteData; 2],
    notes_root_hex: &str,
) -> Result<Vec<u8>, ApiError> {
    use std::str::FromStr;

    let sk0 = element::Element::from_str(&input_notes[0].secret_key)
        .map_err(|e| ApiError::bad_request(format!("invalid secret_key[0]: {e}")))?;
    let sk1 = element::Element::from_str(&input_notes[1].secret_key)
        .map_err(|e| ApiError::bad_request(format!("invalid secret_key[1]: {e}")))?;

    let nullifier_0 = hash::hash_merge([input_notes[0].note.psi, sk0]);
    let nullifier_1 = hash::hash_merge([input_notes[1].note.psi, sk1]);

    let root_normalized = notes_root_hex
        .strip_prefix("0x")
        .unwrap_or(notes_root_hex);
    let root_bytes = hex::decode(root_normalized)
        .map_err(|e| ApiError::bad_request(format!("invalid notes_root hex: {e}")))?;
    if root_bytes.len() != 32 {
        return Err(ApiError::bad_request(format!(
            "notes_root must be 32 bytes, got {}",
            root_bytes.len()
        )));
    }

    let mut smt_blob = Vec::with_capacity(96);
    smt_blob.extend_from_slice(&nullifier_0.to_be_bytes());
    smt_blob.extend_from_slice(&nullifier_1.to_be_bytes());
    smt_blob.extend_from_slice(&root_bytes);

    Ok(smt_blob)
}

/// Generate the SMT inclusion proof server-side from input notes and siblings.
///
/// Builds a [`HyliSmtIncl`] witness and calls the Barretenberg prover.
/// Returns the proof bytes (public_inputs ++ raw_proof) ready for `ProofData`.
async fn generate_smt_proof(
    state: &RouterCtx,
    tx_hash: &TxHash,
    blob_payload: [u8; HYLI_SMT_INCL_BLOB_LENGTH_BYTES],
    identity: Identity,
    tx_blob_count: u32,
    smt_blob_index: u32,
    input_notes: Option<[InputNoteData; 2]>,
    siblings_0: Option<Vec<String>>,
    siblings_1: Option<Vec<String>>,
) -> Result<Vec<u8>, ApiError> {
    use acvm::AcirField;
    use barretenberg::Prove;
    use std::str::FromStr;

    let input_notes = input_notes.ok_or_else(|| {
        ApiError::bad_request(
            "input_notes is required when smt_proof is not provided".to_string(),
        )
    })?;
    let siblings_0_hex = siblings_0.ok_or_else(|| {
        ApiError::bad_request(
            "siblings_0 is required when smt_proof is not provided".to_string(),
        )
    })?;
    let siblings_1_hex = siblings_1.ok_or_else(|| {
        ApiError::bad_request(
            "siblings_1 is required when smt_proof is not provided".to_string(),
        )
    })?;

    if siblings_0_hex.len() != 256 {
        return Err(ApiError::bad_request(format!(
            "siblings_0 must contain exactly 256 elements, got {}",
            siblings_0_hex.len()
        )));
    }
    if siblings_1_hex.len() != 256 {
        return Err(ApiError::bad_request(format!(
            "siblings_1 must contain exactly 256 elements, got {}",
            siblings_1_hex.len()
        )));
    }

    // Convert InputNoteData to zk_primitives::InputNote
    let zk_input_notes: [InputNote; 2] = [
        InputNote::new(
            input_notes[0].note.clone(),
            element::Element::from_str(&input_notes[0].secret_key)
                .map_err(|e| ApiError::bad_request(format!("invalid secret_key[0]: {e}")))?,
        ),
        InputNote::new(
            input_notes[1].note.clone(),
            element::Element::from_str(&input_notes[1].secret_key)
                .map_err(|e| ApiError::bad_request(format!("invalid secret_key[1]: {e}")))?,
        ),
    ];

    // Convert hex sibling strings to element::Base arrays
    let parse_siblings = |hex_strs: &[String]| -> Result<Box<[element::Base; 256]>, ApiError> {
        let mut arr = Box::new([element::Base::zero(); 256]);
        for (i, hex_str) in hex_strs.iter().enumerate() {
            let normalized = hex_str.strip_prefix("0x").unwrap_or(hex_str);
            let bytes = hex::decode(normalized)
                .map_err(|e| ApiError::bad_request(format!("invalid sibling hex [{i}]: {e}")))?;
            arr[i] = element::Base::from_be_bytes_reduce(&bytes);
        }
        Ok(arr)
    };
    let sib_0 = parse_siblings(&siblings_0_hex)?;
    let sib_1 = parse_siblings(&siblings_1_hex)?;

    let job = crate::smt_incl_prover::SmtInclProofJob {
        tx_hash: tx_hash.clone(),
        identity,
        blob: blob_payload,
        tx_blob_count,
        blob_index: smt_blob_index,
        input_notes: zk_input_notes,
        siblings_0: sib_0,
        siblings_1: sib_1,
    };

    let contract_name = state.smt_incl_proof_contract_name.clone();
    let hyli_smt_incl = HyliSmtInclNoirProver::build_hyli_smt_incl(&contract_name, &job)
        .map_err(|e| ApiError::internal(format!("building HyliSmtIncl witness: {e}")))?;

    // Proof generation is CPU-intensive; run in a blocking thread
    let proof = tokio::task::spawn_blocking(move || {
        hyli_smt_incl
            .prove()
            .map_err(|err| format!("generating hyli_smt_incl_proof: {err}"))
    })
    .await
    .map_err(|e| ApiError::internal(format!("proof task panicked: {e}")))?
    .map_err(ApiError::internal)?;

    tracing::info!(%tx_hash, "Server-generated hyli_smt_incl_proof");

    Ok(proof.to_bytes())
}

/// Decode base64 string to bytes
fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    use base64::prelude::*;
    BASE64_STANDARD.decode(input).map_err(|e| e.to_string())
}

// ---- Encrypted Notes Handlers ----

fn validate_tag(tag: &str, field_name: &str) -> Result<(), ApiError> {
    let normalized = tag.strip_prefix("0x").unwrap_or(tag);

    if normalized.is_empty() {
        return Err(ApiError::bad_request(format!(
            "{} must not be empty",
            field_name
        )));
    }

    if normalized.len() != 64 {
        return Err(ApiError::bad_request(format!(
            "{} must be 64 hex characters (32 bytes)",
            field_name
        )));
    }

    if hex::decode(normalized).is_err() {
        return Err(ApiError::bad_request(format!(
            "{} must be valid hexadecimal",
            field_name
        )));
    }

    Ok(())
}

fn validate_ephemeral_pubkey(pubkey: &str) -> Result<(), ApiError> {
    let normalized = pubkey.strip_prefix("0x").unwrap_or(pubkey);

    if normalized.is_empty() {
        return Err(ApiError::bad_request("ephemeral_pubkey must not be empty"));
    }

    if normalized.len() != 64 && normalized.len() != 66 && normalized.len() != 130 {
        return Err(ApiError::bad_request(
            "ephemeral_pubkey must be 64, 66, or 130 hex characters",
        ));
    }

    if hex::decode(normalized).is_err() {
        return Err(ApiError::bad_request(
            "ephemeral_pubkey must be valid hexadecimal",
        ));
    }

    Ok(())
}

async fn upload_note(
    State(state): State<RouterCtx>,
    Json(request): Json<UploadNoteRequest>,
) -> Result<Json<UploadNoteResponse>, ApiError> {
    validate_tag(&request.recipient_tag, "recipient_tag")?;
    validate_ephemeral_pubkey(&request.ephemeral_pubkey)?;

    if let Some(ref sender_tag) = request.sender_tag {
        validate_tag(sender_tag, "sender_tag")?;
    }

    let payload_size = request.encrypted_payload.len();
    if payload_size > state.max_note_payload_size {
        return Err(ApiError::payload_too_large(format!(
            "encrypted_payload exceeds maximum size of {} bytes",
            state.max_note_payload_size
        )));
    }

    if request.encrypted_payload.is_empty() {
        return Err(ApiError::bad_request("encrypted_payload must not be empty"));
    }

    let recipient_tag = request
        .recipient_tag
        .strip_prefix("0x")
        .unwrap_or(&request.recipient_tag)
        .to_lowercase();

    let sender_tag = request.sender_tag.map(|t| {
        t.strip_prefix("0x")
            .unwrap_or(&t)
            .to_lowercase()
            .to_string()
    });

    let ephemeral_pubkey = request
        .ephemeral_pubkey
        .strip_prefix("0x")
        .unwrap_or(&request.ephemeral_pubkey)
        .to_lowercase();

    let (id, stored_at) = state.note_store.insert(
        recipient_tag,
        request.encrypted_payload,
        ephemeral_pubkey,
        sender_tag,
    );

    Ok(Json(UploadNoteResponse { id, stored_at }))
}

async fn get_notes(
    State(state): State<RouterCtx>,
    Path(recipient_tag): Path<String>,
    Query(query): Query<GetNotesQuery>,
) -> Result<Json<GetNotesResponse>, ApiError> {
    validate_tag(&recipient_tag, "recipient_tag")?;

    let normalized_tag = recipient_tag
        .strip_prefix("0x")
        .unwrap_or(&recipient_tag)
        .to_lowercase();

    let (notes, has_more) = state
        .note_store
        .get_notes(&normalized_tag, query.since, query.limit);

    let records: Vec<EncryptedNoteRecord> = notes
        .into_iter()
        .map(|n| EncryptedNoteRecord {
            id: n.id,
            encrypted_payload: n.encrypted_payload,
            ephemeral_pubkey: n.ephemeral_pubkey,
            sender_tag: n.sender_tag,
            stored_at: n.stored_at,
        })
        .collect();

    Ok(Json(GetNotesResponse {
        notes: records,
        has_more,
    }))
}

async fn delete_note(
    State(state): State<RouterCtx>,
    Path((recipient_tag, note_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    validate_tag(&recipient_tag, "recipient_tag")?;

    let normalized_tag = recipient_tag
        .strip_prefix("0x")
        .unwrap_or(&recipient_tag)
        .to_lowercase();

    if state.note_store.delete_note(&normalized_tag, &note_id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("Note not found"))
    }
}

// ---- Address Registry Handlers ----

fn validate_username(username: &str) -> Result<(), ApiError> {
    if username.is_empty() {
        return Err(ApiError::bad_request("username must not be empty"));
    }

    if username.len() > 64 {
        return Err(ApiError::bad_request(
            "username must be at most 64 characters",
        ));
    }

    // Allow alphanumeric, underscore, hyphen
    if !username
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(ApiError::bad_request(
            "username must contain only alphanumeric characters, underscores, or hyphens",
        ));
    }

    Ok(())
}

fn validate_utxo_address(address: &str) -> Result<(), ApiError> {
    let normalized = address.strip_prefix("0x").unwrap_or(address);

    if normalized.is_empty() {
        return Err(ApiError::bad_request("utxo_address must not be empty"));
    }

    if normalized.len() != 64 {
        return Err(ApiError::bad_request(
            "utxo_address must be 64 hex characters (32 bytes)",
        ));
    }

    if hex::decode(normalized).is_err() {
        return Err(ApiError::bad_request(
            "utxo_address must be valid hexadecimal",
        ));
    }

    Ok(())
}

fn normalize_encryption_pubkey(pubkey: &str) -> Result<String, ApiError> {
    let normalized = pubkey.strip_prefix("0x").unwrap_or(pubkey);

    if normalized.is_empty() {
        return Err(ApiError::bad_request("encryption_pubkey must not be empty"));
    }

    if hex::decode(normalized).is_err() {
        return Err(ApiError::bad_request(
            "encryption_pubkey must be valid hexadecimal",
        ));
    }

    let lowered = normalized.to_lowercase();

    match lowered.len() {
        64 => Ok(lowered),
        66 if lowered.starts_with("02") || lowered.starts_with("03") => {
            Ok(lowered[2..].to_string())
        }
        130 if lowered.starts_with("04") => Ok(lowered[2..66].to_string()),
        _ => Err(ApiError::bad_request(
            "encryption_pubkey must be 64 hex chars, or a valid compressed/uncompressed secp256k1 public key",
        )),
    }
}

async fn register_address(
    State(state): State<RouterCtx>,
    Json(request): Json<RegisterAddressRequest>,
) -> Result<Json<RegisterAddressResponse>, ApiError> {
    validate_username(&request.username)?;
    validate_utxo_address(&request.utxo_address)?;
    let encryption_pubkey = normalize_encryption_pubkey(&request.encryption_pubkey)?;

    let utxo_address = request
        .utxo_address
        .strip_prefix("0x")
        .unwrap_or(&request.utxo_address)
        .to_lowercase();

    let previous = state.address_registry.register(
        request.username.clone(),
        utxo_address.clone(),
        encryption_pubkey.clone(),
    );

    // Get the registration we just made
    let registration = state
        .address_registry
        .resolve(&request.username)
        .ok_or_else(|| ApiError::internal("Failed to retrieve registration after insert"))?;

    Ok(Json(RegisterAddressResponse {
        username: registration.username,
        utxo_address: registration.utxo_address,
        encryption_pubkey: registration.encryption_pubkey,
        registered_at: registration.registered_at,
        was_update: previous.is_some(),
    }))
}

async fn resolve_address(
    State(state): State<RouterCtx>,
    Path(username): Path<String>,
) -> Result<Json<ResolveAddressResponse>, ApiError> {
    validate_username(&username)?;

    let registration = state
        .address_registry
        .resolve(&username)
        .ok_or_else(|| ApiError::not_found(format!("Username '{}' not registered", username)))?;

    Ok(Json(ResolveAddressResponse {
        username: registration.username,
        utxo_address: registration.utxo_address,
        encryption_pubkey: registration.encryption_pubkey,
        registered_at: registration.registered_at,
    }))
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn payload_too_large(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": self.message }));
        (self.status, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use sdk::BlobIndex;

    use crate::{api::withdraw_topology, types::TokenTransferRequest};

    use super::normalize_encryption_pubkey;

    #[test]
    fn normalizes_x_coordinate_pubkey() {
        let pubkey = "22b8805cd80e3a297591eb9c43c0ba07fe1165bfed5ace81602d7a9f97d2a830";
        assert_eq!(normalize_encryption_pubkey(pubkey).unwrap(), pubkey);
    }

    #[test]
    fn normalizes_compressed_pubkey() {
        let compressed = "0322b8805cd80e3a297591eb9c43c0ba07fe1165bfed5ace81602d7a9f97d2a830";
        assert_eq!(
            normalize_encryption_pubkey(compressed).unwrap(),
            "22b8805cd80e3a297591eb9c43c0ba07fe1165bfed5ace81602d7a9f97d2a830"
        );
    }

    #[test]
    fn normalizes_uncompressed_pubkey() {
        let uncompressed = concat!(
            "04",
            "22b8805cd80e3a297591eb9c43c0ba07fe1165bfed5ace81602d7a9f97d2a830",
            "5f0f0f2f3f4f5f6f7f8f9fafbfcfdfeff00112233445566778899aabbccddeeff"
        );
        assert_eq!(
            normalize_encryption_pubkey(uncompressed).unwrap(),
            "22b8805cd80e3a297591eb9c43c0ba07fe1165bfed5ace81602d7a9f97d2a830"
        );
    }

    #[test]
    fn rejects_invalid_compressed_prefix() {
        let err = normalize_encryption_pubkey(
            "0522b8805cd80e3a297591eb9c43c0ba07fe1165bfed5ace81602d7a9f97d2a830",
        )
        .unwrap_err();
        assert!(err.message.contains("compressed/uncompressed"));
    }

    #[test]
    fn rejects_invalid_hex() {
        let err = normalize_encryption_pubkey("zz").unwrap_err();
        assert_eq!(err.message, "encryption_pubkey must be valid hexadecimal");
    }

    #[test]
    fn withdraw_topology_adds_token_callee_and_state_caller() {
        let token_transfer = TokenTransferRequest {
            token_contract: "oranj".into(),
            sender: "hyli-utxo-state".into(),
            recipient: "0xwallet".into(),
            amount: 42,
        };

        let (state_callees, token_caller) =
            withdraw_topology("hyli-utxo-state", Some(&token_transfer));

        assert_eq!(state_callees, vec![BlobIndex(2), BlobIndex(3)]);
        assert_eq!(token_caller, Some(BlobIndex(0)));
    }

    #[test]
    fn non_withdraw_topology_only_references_smt_blob() {
        let token_transfer = TokenTransferRequest {
            token_contract: "oranj".into(),
            sender: "alice@wallet".into(),
            recipient: "hyli-utxo-state".into(),
            amount: 42,
        };

        let (state_callees, token_caller) =
            withdraw_topology("hyli-utxo-state", Some(&token_transfer));

        assert_eq!(state_callees, vec![BlobIndex(2)]);
        assert_eq!(token_caller, None);
    }
}
