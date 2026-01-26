use std::sync::Arc;

use crate::{
    app::{build_note, FaucetMintCommand, FAUCET_MINT_AMOUNT},
    metrics::FaucetMetrics,
    note_store::NoteStore,
    types::{
        EncryptedNoteRecord, FaucetRequest, FaucetResponse, GetNotesQuery, GetNotesResponse,
        TransferRequest, TransferResponse, UploadNoteRequest, UploadNoteResponse,
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
use hyli_modules::{
    bus::{BusClientSender, SharedMessageBus},
    module_bus_client, module_handle_messages,
    modules::{BuildApiContextInner, Module},
};
use client_sdk::rest_client::NodeApiHttpClient;
use sdk::ContractName;
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};

pub struct ApiModule {
    bus: ApiModuleBusClient,
}

pub struct ApiModuleCtx {
    pub api: Arc<BuildApiContextInner>,
    pub default_amount: u64,
    pub contract_name: ContractName,
    pub metrics: FaucetMetrics,
    pub note_store: Arc<NoteStore>,
    pub max_note_payload_size: usize,
    pub client: NodeApiHttpClient,
    pub utxo_contract_name: String,
    pub utxo_state_contract_name: String,
}

#[derive(Clone)]
struct RouterCtx {
    default_amount: u64,
    bus: ApiModuleBusClient,
    metrics: FaucetMetrics,
    note_store: Arc<NoteStore>,
    max_note_payload_size: usize,
    client: NodeApiHttpClient,
    utxo_contract_name: String,
    utxo_state_contract_name: String,
}

module_bus_client! {
    #[derive(Debug)]
    pub struct ApiModuleBusClient {
        sender(FaucetMintCommand),
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
            max_note_payload_size: ctx.max_note_payload_size,
            client: ctx.client.clone(),
            utxo_contract_name: ctx.utxo_contract_name.clone(),
            utxo_state_contract_name: ctx.utxo_state_contract_name.clone(),
        };

        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET, Method::POST, Method::DELETE])
            .allow_headers(Any);

        let router = Router::new()
            .route("/_health", get(health))
            .route("/api/faucet", post(faucet))
            .route("/api/transfer", post(transfer))
            .route("/api/notes", post(upload_note))
            .route("/api/notes/{recipient_tag}", get(get_notes))
            .route("/api/notes/{recipient_tag}/{note_id}", delete(delete_note))
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

async fn transfer(
    State(_state): State<RouterCtx>,
    Json(_request): Json<TransferRequest>,
) -> Result<Json<TransferResponse>, ApiError> {
    // Placeholder for transfer logic
    Err(ApiError::internal("Transfer not yet implemented"))
}

// ---- Encrypted Notes Handlers ----

fn validate_tag(tag: &str, field_name: &str) -> Result<(), ApiError> {
    let normalized = tag.strip_prefix("0x").unwrap_or(tag);

    if normalized.is_empty() {
        return Err(ApiError::bad_request(format!("{} must not be empty", field_name)));
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

    let sender_tag = request
        .sender_tag
        .map(|t| t.strip_prefix("0x").unwrap_or(&t).to_lowercase().to_string());

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
