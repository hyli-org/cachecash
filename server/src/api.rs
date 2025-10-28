use std::sync::Arc;

use crate::{
    app::{build_note, FaucetMintCommand, FAUCET_MINT_AMOUNT},
    metrics::FaucetMetrics,
    types::{FaucetRequest, FaucetResponse},
};
use anyhow::Result;
use axum::{
    extract::State,
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use hyli_modules::{
    bus::{BusClientSender, SharedMessageBus},
    module_bus_client, module_handle_messages,
    modules::{BuildApiContextInner, Module},
};
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
}

#[derive(Clone)]
struct RouterCtx {
    default_amount: u64,
    bus: ApiModuleBusClient,
    metrics: FaucetMetrics,
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
        };

        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET, Method::POST])
            .allow_headers(Any);

        let router = Router::new()
            .route("/_health", get(health))
            .route("/api/faucet", post(faucet))
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
