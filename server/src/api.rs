use crate::{
    app::{FaucetMintRequest, FAUCET_MINT_AMOUNT},
    keys::derive_key_material,
    tx::{FAUCET_IDENTITY_PREFIX, HYLI_UTXO_CONTRACT_NAME},
    types::{FaucetRequest, FaucetResponse, KeyPairInfo},
};
use axum::{
    extract::{Path, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use hex::encode as hex_encode;
use sdk::{BlobTransaction, Hashed, Identity};
use serde_json::json;
use tokio::sync::broadcast::Sender;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

#[derive(Clone)]
pub struct ApiState {
    pub default_amount: u64,
    pub faucet_sender: Sender<FaucetMintRequest>,
}

pub fn build_router(state: ApiState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers(Any);

    Router::new()
        .route("/_health", get(health))
        .route("/api/faucet/{username}", get(faucet_log))
        .route("/api/faucet", post(faucet))
        .with_state(state)
        .layer(cors)
}

async fn health() -> &'static str {
    "OK"
}

async fn faucet(
    State(state): State<ApiState>,
    Json(request): Json<FaucetRequest>,
) -> Result<Json<FaucetResponse>, ApiError> {
    let name = request.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name must not be empty"));
    }

    let amount = FAUCET_MINT_AMOUNT;

    let key_material =
        derive_key_material(name).map_err(|err| ApiError::bad_request(err.to_string()))?;

    state
        .faucet_sender
        .send(FaucetMintRequest {
            key_material: key_material.clone(),
            amount,
        })
        .map_err(|err| ApiError::internal(err.to_string()))?;

    let placeholder_identity = Identity(format!(
        "{}@{}",
        FAUCET_IDENTITY_PREFIX, HYLI_UTXO_CONTRACT_NAME
    ));
    let placeholder_tx = BlobTransaction::new(placeholder_identity, vec![]);
    let placeholder_hash = placeholder_tx.hashed();

    let response = FaucetResponse {
        name: name.to_string(),
        key_pair: KeyPairInfo {
            private_key_hex: hex_encode(key_material.private_key),
            public_key_hex: hex_encode(key_material.public_key),
        },
        contract_name: HYLI_UTXO_CONTRACT_NAME.to_string(),
        amount,
        tx_hash: placeholder_hash.0,
        transaction: placeholder_tx,
    };

    Ok(Json(response))
}

async fn faucet_log(
    Path(username): Path<String>,
    State(state): State<ApiState>,
) -> Json<serde_json::Value> {
    info!(
        user = %username,
        default_amount = state.default_amount,
        contract = HYLI_UTXO_CONTRACT_NAME,
        "Faucet endpoint invoked for user"
    );

    Json(json!({
        "message": format!("Faucet request received for {username}"),
        "default_amount": state.default_amount,
        "contract_name": HYLI_UTXO_CONTRACT_NAME,
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
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": self.message }));
        (self.status, body).into_response()
    }
}
