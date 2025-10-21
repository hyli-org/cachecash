use crate::{
    keys::derive_key_material,
    tx::build_faucet_transaction,
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
use sdk::{ContractName, Hashed};
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

#[derive(Clone)]
pub struct ApiState {
    pub contract_name: ContractName,
    pub default_amount: u64,
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

    let amount = request.amount.unwrap_or(state.default_amount);
    if amount == 0 {
        return Err(ApiError::bad_request("amount must be greater than zero"));
    }

    let key_material =
        derive_key_material(name).map_err(|err| ApiError::bad_request(err.to_string()))?;

    let transaction = build_faucet_transaction(
        &state.contract_name,
        key_material.public_key.clone(),
        amount,
    );
    let tx_hash = transaction.hashed();

    let response = FaucetResponse {
        name: name.to_string(),
        key_pair: KeyPairInfo {
            private_key_hex: hex_encode(key_material.private_key),
            public_key_hex: hex_encode(key_material.public_key),
        },
        contract_name: state.contract_name.0.clone(),
        amount,
        tx_hash: tx_hash.0,
        transaction,
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
        contract = %state.contract_name.0,
        "Faucet endpoint invoked for user"
    );

    Json(json!({
        "message": format!("Faucet request received for {username}"),
        "default_amount": state.default_amount,
        "contract_name": state.contract_name.0,
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
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": self.message }));
        (self.status, body).into_response()
    }
}
