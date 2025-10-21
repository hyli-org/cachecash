use std::path::PathBuf;

use self::cli::CliArgs;
use crate::Mode;
use color_eyre::Result;
use dirs::home_dir;
use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use primitives::peer::PeerIdSigner;
use serde::Deserialize;
use std::io::Read;
use std::{fs::File, str::FromStr};

pub mod cli;

// TODO: should we use kebab-case? Currently _ is used to split into
// multiple level dictionaries
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    /// Sentry DSN URL
    pub sentry_dsn: Option<String>,

    /// Sentry environment name
    pub env_name: String,

    /// Maximum number of txns to include in a block
    pub block_txns_count: usize,

    /// Minimum block duration in seconds
    pub min_block_duration: usize,

    pub sync_chunk_size: u64,

    pub sync_timeout_ms: u64,

    pub fast_sync_threshold: u64,

    pub mode: Mode,

    /// Private key of validator
    pub secret_key: PeerIdSigner,

    /// RPC config
    pub rpc_laddr: String,

    /// P2P config
    pub p2p: ::p2p2::Config,

    /// Path to the database
    pub db_path: PathBuf,

    /// Path to Smirk
    pub smirk_path: PathBuf,

    pub eth_rpc_url: String,

    pub rollup_contract_addr: String,

    /// If the last commit is older than this, health check will fail
    pub health_check_commit_interval_sec: u64,

    pub rollup_wait_time_ms: u64,

    /// Optional postgres database for synchronization between provers
    pub prover_database_url: Option<String>,

    /// Blocks that should not be validated or rolled up
    pub bad_blocks: Vec<u64>,

    /// The minimum amount of gas (in gwei) to use for transactions
    pub minimum_gas_price_gwei: Option<u64>,

    pub safe_eth_height_offset: u64,
}

impl Config {
    /// The text of the default config string
    pub const DEFAULT_STR: &str = include_str!("./default_config.toml");

    /// Load a [`Config`] from a file and environment
    ///
    /// `config_path` doesn't need to point to an actual file
    pub fn from_env(args: CliArgs) -> Result<Self> {
        let mut config: Config = Figment::new()
            .merge(Toml::file(args.config_path))
            .merge(
                Env::prefixed("POLY_")
                    .split("__")
                    .map(|k| k.as_str().replace('_', "-").into()),
            )
            .join(Toml::string(Self::DEFAULT_STR))
            .extract()?;

        if let Some(mode) = args.mode {
            config.mode = mode;
        }

        if let Some(p2p_laddr) = args.p2p_laddr {
            config.p2p.laddr = p2p_laddr;
        }

        if let Some(p2p_dial) = args.p2p_dial {
            config.p2p.dial = p2p_dial;
        }

        if let Some(secret_key_path) = args.secret_key_path {
            let mut file = File::open(secret_key_path)?;
            let mut key = String::new();
            file.read_to_string(&mut key)?;
            config.secret_key = PeerIdSigner::from_str(&key)?;
        }

        if let Some(secret_key) = args.secret_key {
            config.secret_key = secret_key;
        }

        if let Some(rpc_laddr) = args.rpc_laddr {
            config.rpc_laddr = rpc_laddr;
        }

        if let Some(db_path) = args.db_path {
            config.db_path = db_path;
        }

        if config.db_path.starts_with("~") {
            config.db_path = home_dir()
                .unwrap()
                .join(config.db_path.strip_prefix("~").unwrap());
        }

        if let Some(smirk_path) = args.smirk_path {
            config.smirk_path = smirk_path;
        }

        if config.smirk_path.starts_with("~") {
            config.smirk_path = home_dir()
                .unwrap()
                .join(config.smirk_path.strip_prefix("~").unwrap());
        }

        if let Some(eth_rpc_url) = args.eth_rpc_url {
            config.eth_rpc_url = eth_rpc_url;
        }

        if let Some(rollup_contract_addr) = args.rollup_contract_addr {
            config.rollup_contract_addr = rollup_contract_addr;
        }

        if let Some(sync_chunk_size) = args.sync_chunk_size {
            config.sync_chunk_size = sync_chunk_size;
        }

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn can_parse_from_empty() {
        let args = CliArgs::try_parse_from(["node"]).unwrap();
        Config::from_env(args).unwrap();
    }
}
