use std::{path::PathBuf, sync::Arc};

use anyhow::{anyhow, Context, Result};
use axum::Router;
use clap::Parser;
use client_sdk::{
    helpers::sp1::SP1Prover,
    rest_client::{NodeApiClient, NodeApiHttpClient},
};
use hyli_modules::{
    bus::{metrics::BusMetrics, SharedMessageBus},
    modules::{
        block_processor::NodeStateBlockProcessor,
        da_listener::{SignedDAListener, DAListenerConf},
        prover::{AutoProver, AutoProverCtx},
        rest::{RestApi, RestApiRunContext},
        BuildApiContextInner, ModulesHandler,
    },
    telemetry::init_prometheus_registry_meter_provider,
};
use sdk::{api::NodeInfo, verifiers, ContractName, Verifier};
use server::{
    api::{ApiModule, ApiModuleCtx},
    app::{FaucetApp, FaucetAppContext},
    conf::Conf,
    hyli_utxo_state_client::HyliUtxoStateExecutor,
    init::{hyli_utxo_noir_deployment, hyli_utxo_state_deployment, init_node, ContractInit},
    metrics::FaucetMetrics,
    noir_prover::{HyliUtxoNoirProver, HyliUtxoNoirProverCtx},
    note_store::{AddressRegistry, NoteStore},
    utils::load_utxo_state_proving_key,
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
        config.utxo_contract_name = contract_name;
    }

    init_tracing(&config.log_format)
        .with_context(|| "initializing tracing subscriber".to_string())?;

    let registry =
        init_prometheus_registry_meter_provider().context("starting prometheus exporter")?;

    let faucet_metrics = FaucetMetrics::global(config.id.clone());

    let node_client = Arc::new(
        NodeApiHttpClient::new(config.node_url.clone()).context("creating node REST client")?,
    );

    let hyli_utxo_contract = hyli_utxo_noir_deployment(&config.utxo_contract_name);
    let hyli_utxo_state_contract = hyli_utxo_state_deployment(&config.utxo_state_contract_name);
    let contracts = vec![
        ContractInit {
            deployment: hyli_utxo_contract.clone(),
            verifier: Verifier(verifiers::NOIR.to_string()),
        },
        ContractInit {
            deployment: hyli_utxo_state_contract.clone(),
            verifier: Verifier(verifiers::SP1_4.to_string()),
        },
    ];
    init_node(node_client.as_ref(), &contracts)
        .await
        .context("initializing contracts on node")?;

    let data_directory = PathBuf::from(&config.data_directory);
    std::fs::create_dir_all(&data_directory).context("creating data directory")?;

    let proving_key = load_utxo_state_proving_key(&data_directory)
        .context("loading hyli-utxo-state proving key")?;
    let prover = Arc::new(SP1Prover::new(proving_key).await);

    let shared_bus = SharedMessageBus::new(BusMetrics::global());
    let mut handler = ModulesHandler::new(&shared_bus, data_directory.clone()).await;

    handler
        .build_module::<HyliUtxoNoirProver>(Arc::new(HyliUtxoNoirProverCtx {
            node: node_client.clone() as Arc<dyn NodeApiClient + Send + Sync>,
            contract: hyli_utxo_contract.clone(),
            verify_locally: true,
            metrics: faucet_metrics.clone(),
        }))
        .await
        .context("building hyli_utxo Noir prover module")?;

    handler
        .build_module::<FaucetApp>(FaucetAppContext {
            client: node_client.as_ref().clone(),
            utxo_contract_name: config.utxo_contract_name.clone(),
            utxo_state_contract_name: config.utxo_state_contract_name.clone(),
        })
        .await
        .context("building faucet module")?;

    let api_builder_ctx = Arc::new(BuildApiContextInner {
        router: std::sync::Mutex::new(Some(Router::new())),
        openapi: Default::default(),
    });

    let note_store = if config.persist_encrypted_notes {
        let notes_path = data_directory.join("encrypted_notes.json");
        Arc::new(
            NoteStore::with_persistence(None, notes_path.to_string_lossy().to_string())
                .context("initializing note store with persistence")?,
        )
    } else {
        Arc::new(NoteStore::new(None))
    };

    // Address registry for username -> UTXO address resolution
    // Uses same persistence setting as encrypted notes
    let address_registry = if config.persist_encrypted_notes {
        let registry_path = data_directory.join("address_registry.json");
        Arc::new(
            AddressRegistry::with_persistence(registry_path.to_string_lossy().to_string())
                .context("initializing address registry with persistence")?,
        )
    } else {
        Arc::new(AddressRegistry::new())
    };

    handler
        .build_module::<ApiModule>(Arc::new(ApiModuleCtx {
            api: api_builder_ctx.clone(),
            default_amount: config.default_faucet_amount,
            contract_name: ContractName(config.utxo_contract_name.clone()),
            metrics: faucet_metrics.clone(),
            note_store,
            address_registry,
            max_note_payload_size: config.max_note_payload_size,
            client: node_client.as_ref().clone(),
            utxo_contract_name: config.utxo_contract_name.clone(),
            utxo_state_contract_name: config.utxo_state_contract_name.clone(),
        }))
        .await
        .context("building API module")?;

    handler
        .build_module::<SignedDAListener<NodeStateBlockProcessor>>(DAListenerConf {
            start_block: None,
            data_directory: data_directory.clone(),
            da_read_from: config.da_read_from.clone(),
            timeout_client_secs: 10,
            da_fallback_addresses: vec![],
            processor_config: (),
        })
        .await
        .context("building DA listener module")?;

    handler
        .build_module::<AutoProver<HyliUtxoStateExecutor, SP1Prover>>(Arc::new(AutoProverCtx {
            data_directory: data_directory.clone(),
            prover: prover.clone(),
            contract_name: ContractName(config.utxo_state_contract_name.clone()),
            node: node_client.clone() as Arc<dyn NodeApiClient + Send + Sync>,
            default_state: HyliUtxoStateExecutor::default(),
            buffer_blocks: config.buffer_blocks,
            max_txs_per_proof: config.max_txs_per_proof,
            tx_working_window_size: config.tx_working_window_size,
            api: Some(api_builder_ctx.clone()),
        }))
        .await
        .context("building auto prover module")?;

    let mut router_guard = api_builder_ctx
        .router
        .lock()
        .expect("API router mutex poisoned");
    let router = router_guard.take().unwrap_or_else(Router::new);
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
            da_address: config.da_read_from.clone(),
            pubkey: None,
        },
        router,
        config.rest_server_max_body_size,
        openapi,
    )
    .with_registry(registry.clone());

    handler
        .build_module::<RestApi>(rest_context)
        .await
        .context("building REST API module")?;

    info!(
        contract = %config.utxo_contract_name,
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
