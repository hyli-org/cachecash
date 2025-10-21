use clap::Parser;
use contracts::{ConfirmationType, RollupContract, U256};
use eyre::ContextCompat;
use rpc::tracing::{LogFormat, LogLevel};
use serde::{Deserialize, Serialize};
use std::{str::FromStr, time::Duration};

#[derive(Parser, Debug, Serialize, Deserialize, Clone)]
#[clap(name = "Polybase Burn Subsitutor")]
#[command(author = "Polybase <hello@polybase.xyz>")]
#[command(author, version, about = "Polybase Burn Subsitutor - enables instant withdrawals", long_about = None)]
#[command(propagate_version = true)]
pub struct Config {
    /// Log level
    #[arg(value_enum, long, env = "LOG_LEVEL", default_value = "INFO")]
    log_level: LogLevel,

    /// Log format
    #[arg(value_enum, long, env = "LOG_FORMAT", default_value = "PRETTY")]
    log_format: LogFormat,

    #[arg(
        long,
        env = "ROLLUP_CONTRACT_ADDRESS",
        default_value = "0xcf7ed3acca5a467e9e704c703e8d87f634fb0fc9"
    )]
    rollup_contract_address: String,

    #[arg(
        long,
        env = "USDC_CONTRACT_ADDRESS",
        default_value = "0x5fbdb2315678afecb367f032d93f642f64180aa3"
    )]
    usdc_contract_address: String,

    #[arg(
        long,
        env = "EVM_SECRET_KEY",
        default_value = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
    )]
    evm_secret_key: String,

    #[arg(long, env = "EVM_RPC_URL", default_value = "http://localhost:8545")]
    evm_rpc_url: String,

    #[arg(long, env = "NODE_RPC_URL", default_value = "http://localhost:8080")]
    node_rpc_url: String,

    #[arg(long, env = "MINIMUM_GAS_PRICE_GWEI")]
    minimum_gas_price_gwei: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<(), eyre::Error> {
    let config = Config::parse();

    rpc::tracing::setup_tracing(
        &[
            "burn_substitutor",
            "node",
            "solid",
            "smirk",
            "p2p",
            "prover",
            "zk_primitives",
            "contracts",
            "block_store",
            "element",
        ],
        &config.log_level,
        &config.log_format,
        std::env::var("SENTRY_DSN").ok(),
        std::env::var("ENV_NAME").unwrap_or_else(|_| "dev".to_owned()),
    )?;

    let secret_key = contracts::SecretKey::from_str(
        config
            .evm_secret_key
            .strip_prefix("0x")
            .context("Secret key must start with 0x")?,
    )?;

    let client = contracts::Client::new(&config.evm_rpc_url, config.minimum_gas_price_gwei);
    let rollup_contract = RollupContract::load(
        client.clone(),
        137,
        &config.rollup_contract_address,
        secret_key,
    )
    .await?;
    let usdc_contract = contracts::USDCContract::load(
        client.clone(),
        137,
        &config.usdc_contract_address,
        secret_key,
    )
    .await?;

    if usdc_contract
        .allowance(rollup_contract.signer_address, rollup_contract.address())
        .await?
        != U256::MAX
    {
        let approve_txn = usdc_contract.approve_max(rollup_contract.address()).await?;
        client
            .wait_for_confirm(
                approve_txn,
                Duration::from_secs(1),
                ConfirmationType::Latest,
            )
            .await?;
    }

    let mut substitutor = burn_substitutor::BurnSubstitutor::new(
        rollup_contract,
        usdc_contract,
        config.node_rpc_url,
        Duration::from_secs(1),
    );

    tracing::info!("Starting burn substitutor");

    loop {
        let substitutions = substitutor.tick().await?;
        for nullifier in &substitutions {
            tracing::info!(?nullifier, "Substituted burn");
        }

        if substitutions.is_empty() {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}
