use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use axum::Router;
use clap::Parser;
use client_sdk::rest_client::NodeApiHttpClient;
use hyli_modules::{
    bus::{metrics::BusMetrics, SharedMessageBus},
    modules::{
        rest::{RestApi, RestApiRunContext},
        BuildApiContextInner, ModulesHandler,
    },
};
use sdk::{api::NodeInfo, verifiers, ContractName, Verifier};
use server::{
    api::{ApiModule, ApiModuleCtx},
    app::{FaucetApp, FaucetAppContext},
    conf::Conf,
    init::{
        hyli_utxo_noir_deployment, hyli_utxo_state_deployment, init_node, ContractInit,
    },
    tx::HYLI_UTXO_CONTRACT_NAME,
};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(version, about = "Run the zfruit faucet server", long_about = None)]
struct Args {
    #[arg(long, default_value = "config.toml")]
    config_file: Vec<String>,

    /// Override the server port defined in the configuration file.
    #[arg(long)]
    server_port: Option<u16>,

    /// Override the default faucet amount defined in the configuration file.
    #[arg(long)]
    faucet_amount: Option<u64>,

    /// Override the Noir contract name used to build transactions.
    #[arg(long)]
    contract_name: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let mut config = Conf::new(args.config_file).context("reading configuration")?;

    if let Some(port) = args.server_port {
        config.rest_server_port = port;
    }
    if let Some(amount) = args.faucet_amount {
        config.default_faucet_amount = amount;
    }
    if let Some(contract_name) = args.contract_name {
        config.contract_name = contract_name;
    }

    if config.contract_name != HYLI_UTXO_CONTRACT_NAME {
        config.contract_name = HYLI_UTXO_CONTRACT_NAME.to_string();
    }

    init_tracing(&config.log_format)
        .with_context(|| "initializing tracing subscriber".to_string())?;

    let node_client =
        NodeApiHttpClient::new(config.node_url.clone()).context("creating node REST client")?;

    let contracts = vec![
        ContractInit {
            deployment: hyli_utxo_noir_deployment(),
            verifier: Verifier(verifiers::NOIR.to_string()),
        },
        ContractInit {
            deployment: hyli_utxo_state_deployment(),
            verifier: Verifier(verifiers::SP1_4.to_string()),
        },
    ];
    init_node(&node_client, &contracts)
        .await
        .context("initializing contracts on node")?;

    let shared_bus = SharedMessageBus::new(BusMetrics::global(config.id.clone()));
    let mut handler = ModulesHandler::new(&shared_bus).await;

    handler
        .build_module::<FaucetApp>(FaucetAppContext {
            client: node_client.clone(),
        })
        .await
        .context("building faucet module")?;

    let api_builder_ctx = Arc::new(BuildApiContextInner {
        router: std::sync::Mutex::new(Some(Router::new())),
        openapi: Default::default(),
    });

    handler
        .build_module::<ApiModule>(Arc::new(ApiModuleCtx {
            api: api_builder_ctx.clone(),
            default_amount: config.default_faucet_amount,
            contract_name: ContractName(config.contract_name.clone()),
        }))
        .await
        .context("building API module")?;

    let mut router_guard = api_builder_ctx
        .router
        .lock()
        .expect("API router mutex poisoned");
    let router = router_guard
        .take()
        .unwrap_or_else(Router::new);
    drop(router_guard);

    let openapi = api_builder_ctx
        .openapi
        .lock()
        .expect("OpenAPI mutex poisoned")
        .clone();

    let rest_context = RestApiRunContext::new(
        config.rest_server_port,
        NodeInfo {
            id: config.id.clone(),
            da_address: config.node_url.clone(),
            pubkey: None,
        },
        router,
        config.rest_server_max_body_size,
        openapi,
    );

    handler
        .build_module::<RestApi>(rest_context)
        .await
        .context("building REST API module")?;

    info!(
        contract = %config.contract_name,
        amount = config.default_faucet_amount,
        port = config.rest_server_port,
        "Starting zfruit faucet server",
    );

    handler.start_modules().await.context("starting modules")?;
    handler
        .exit_process()
        .await
        .context("waiting for module shutdown")?;

    Ok(())
}

fn init_tracing(log_format: &str) -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let builder = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false);

    if log_format.eq_ignore_ascii_case("json") {
        builder
            .json()
            .try_init()
            .map_err(|err| anyhow!("failed to initialise tracing: {err}"))
    } else if log_format.eq_ignore_ascii_case("compact") {
        builder
            .compact()
            .try_init()
            .map_err(|err| anyhow!("failed to initialise tracing: {err}"))
    } else {
        builder
            .try_init()
            .map_err(|err| anyhow!("failed to initialise tracing: {err}"))
    }
}
