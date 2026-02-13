use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use axum::Router;
use clap::Parser;
use client_sdk::{
    helpers::sp1::SP1Prover,
    rest_client::{NodeApiClient, NodeApiHttpClient},
};
use hyli_modules::{
    bus::SharedMessageBus,
    modules::{
        block_processor::NodeStateBlockProcessor,
        da_listener::{DAListenerConf, SignedDAListener},
        prover::{AutoProver, AutoProverCtx},
        rest::{RestApi, RestApiRunContext},
        BuildApiContextInner, ModulesHandler,
    },
    utils::logger::setup_otlp,
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
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(version, about = "Run the zfruit faucet server", long_about = None)]
struct Args {
    #[arg(long, default_value = "config.toml")]
    config_file: Vec<String>,

    /// Override the default faucet amount defined in the configuration file.
    #[arg(long)]
    faucet_amount: Option<u64>,

    /// Override the Noir contract name used to build transactions.
    #[arg(long)]
    contract_name: Option<String>,

    #[arg(long, default_value = "false")]
    pub tracing: bool,

    /// Clean the data directory before starting the server
    /// Argument used by hylix tests commands
    #[arg(long, default_value = "false")]
    pub clean_data_directory: bool,

    /// Server port (overrides config)
    /// Argument used by hylix tests commands
    #[arg(long)]
    pub server_port: Option<u16>,
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

    setup_otlp(&config.log_format, "cachecache".to_string(), args.tracing)
        .with_context(|| "initializing tracing subscriber".to_string())?;

    if args.clean_data_directory && std::fs::exists(&config.data_directory).unwrap_or(false) {
        info!("Cleaning data directory: {:?}", &config.data_directory);
        std::fs::remove_dir_all(&config.data_directory).context("cleaning data directory")?;
    }

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

    let shared_bus = SharedMessageBus::new();
    let mut handler = match ModulesHandler::new(&shared_bus, data_directory.clone()) {
        Ok(h) => h,
        Err(e) => {
            error!("error: {:?}", e);
            anyhow::bail!("failed to initialize modules handler");
        }
    };

    let proving_key = load_utxo_state_proving_key(&data_directory)
        .context("loading hyli-utxo-state proving key")?;
    let prover = Arc::new(SP1Prover::new(proving_key).await);

    handler
        .build_module::<HyliUtxoNoirProver>(Arc::new(HyliUtxoNoirProverCtx {
            node: node_client.clone() as Arc<dyn NodeApiClient + Send + Sync>,
            contract: hyli_utxo_contract.clone(),
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
    );

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
