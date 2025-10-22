use std::sync::Arc;

use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use hex::encode as hex_encode;
use hyli_modules::{
    bus::{BusClientSender, SharedMessageBus},
    module_bus_client, module_handle_messages,
    modules::{BuildApiContextInner, Module},
};
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::{
    app::{FaucetMintRequest, FAUCET_MINT_AMOUNT},
    keys::derive_key_material,
    tx::FAUCET_IDENTITY_PREFIX,
    types::{FaucetRequest, FaucetResponse, KeyPairInfo},
};
use sdk::{BlobTransaction, ContractName, Hashed, Identity};

pub struct ApiModule {
    bus: ApiModuleBusClient,
}

pub struct ApiModuleCtx {
    pub api: Arc<BuildApiContextInner>,
    pub default_amount: u64,
    pub contract_name: ContractName,
}

#[derive(Clone)]
struct RouterCtx {
    default_amount: u64,
    contract_name: String,
    bus: ApiModuleBusClient,
}

module_bus_client! {
    #[derive(Debug)]
    pub struct ApiModuleBusClient {
        sender(FaucetMintRequest),
    }
}

impl Module for ApiModule {
    type Context = Arc<ApiModuleCtx>;

    async fn build(bus: SharedMessageBus, ctx: Self::Context) -> Result<Self> {
        let module_bus = ApiModuleBusClient::new_from_bus(bus.new_handle()).await;

        let router_ctx = RouterCtx {
            default_amount: ctx.default_amount,
            contract_name: ctx.contract_name.0.clone(),
            bus: module_bus.clone(),
        };

        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET, Method::POST])
            .allow_headers(Any);

        let router = Router::new()
            .route("/_health", get(health))
            .route("/api/faucet/{username}", get(faucet_log))
            .route("/api/faucet", post(faucet))
            .with_state(router_ctx)
            .layer(cors);

        if let Ok(mut guard) = ctx.api.router.lock() {
            let merged = guard
                .take()
                .unwrap_or_else(Router::new)
                .merge(router);
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
        contract_name,
        mut bus,
    } = state;

    let name = request.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name must not be empty"));
    }

    let default_amount = if default_amount == 0 {
        FAUCET_MINT_AMOUNT
    } else {
        default_amount
    };
    let amount = request.amount.unwrap_or(default_amount);
    if amount == 0 {
        return Err(ApiError::bad_request("amount must be greater than zero"));
    }

    let key_material =
        derive_key_material(name).map_err(|err| ApiError::bad_request(err.to_string()))?;

    bus.send(FaucetMintRequest {
        key_material: key_material.clone(),
        amount,
    })
    .map_err(|err| ApiError::internal(err.to_string()))?;

    let placeholder_identity =
        Identity(format!("{}@{}", FAUCET_IDENTITY_PREFIX, contract_name));
    let placeholder_tx = BlobTransaction::new(
        placeholder_identity.clone(),
        vec![],
    );
    let placeholder_hash = placeholder_tx.hashed();

    let response = FaucetResponse {
        name: name.to_string(),
        key_pair: KeyPairInfo {
            private_key_hex: hex_encode(key_material.private_key),
            public_key_hex: hex_encode(key_material.public_key),
        },
        contract_name: contract_name.clone(),
        amount,
        tx_hash: placeholder_hash.0,
        transaction: placeholder_tx,
    };

    Ok(Json(response))
}

async fn faucet_log(
    Path(username): Path<String>,
    State(state): State<RouterCtx>,
) -> Json<serde_json::Value> {
    info!(
        user = %username,
        default_amount = state.default_amount,
        contract = state.contract_name,
        "Faucet endpoint invoked for user"
    );

    Json(json!({
        "message": format!("Faucet request received for {username}"),
        "default_amount": state.default_amount,
        "contract_name": state.contract_name,
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
