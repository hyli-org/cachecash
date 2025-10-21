use std::{io::ErrorKind, net::SocketAddr};

use anyhow::{anyhow, Context, Result};
use axum::Router;
use clap::Parser;
use sdk::ContractName;
use server::{
    api::{build_router, ApiState},
    conf::Conf,
};
use tokio::{net::TcpListener, signal};
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

    init_tracing(&config.log_format)
        .with_context(|| "initializing tracing subscriber".to_string())?;

    let state = ApiState {
        contract_name: ContractName(config.contract_name.clone()),
        default_amount: config.default_faucet_amount,
    };

    let router: Router = build_router(state);

    let (listener, addr) = bind_listener(config.rest_server_port).await?;

    info!(
        contract = %config.contract_name,
        amount = config.default_faucet_amount,
        address = %addr,
        "Starting zfruit faucet server",
    );

    axum::serve(listener, router.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("running HTTP server")
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

async fn shutdown_signal() {
    if let Err(err) = signal::ctrl_c().await {
        info!(error = %err, "Failed to listen for shutdown signal");
    } else {
        info!("Shutdown signal received, stopping server");
    }
}

async fn bind_listener(port: u16) -> Result<(TcpListener, SocketAddr)> {
    let wildcard_addr = SocketAddr::from(([0, 0, 0, 0], port));

    match TcpListener::bind(wildcard_addr).await {
        Ok(listener) => Ok((listener, wildcard_addr)),
        Err(err) if err.kind() == ErrorKind::PermissionDenied => {
            let loopback_addr = SocketAddr::from(([127, 0, 0, 1], port));
            let listener = TcpListener::bind(loopback_addr)
                .await
                .with_context(|| format!("binding HTTP listener on {loopback_addr}"))?;

            info!(
                original_address = %wildcard_addr,
                fallback_address = %loopback_addr,
                "Operation not permitted on wildcard address, falling back to loopback"
            );

            Ok((listener, loopback_addr))
        }
        Err(err) => Err(err).with_context(|| format!("binding HTTP listener on {wildcard_addr}")),
    }
}
