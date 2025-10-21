use config::{Config, Environment, File};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Conf {
    pub id: String,
    pub log_format: String,
    pub rest_server_port: u16,
    pub rest_server_max_body_size: usize,
    pub contract_name: String,
    pub default_faucet_amount: u64,
}

impl Conf {
    pub fn new(config_files: Vec<String>) -> Result<Self, anyhow::Error> {
        let mut builder = Config::builder().add_source(File::from_str(
            include_str!("conf_defaults.toml"),
            config::FileFormat::Toml,
        ));

        for config_file in config_files {
            builder = builder.add_source(File::with_name(&config_file).required(false));
        }

        builder
            .add_source(
                Environment::with_prefix("zfruit")
                    .separator("__")
                    .prefix_separator("_")
                    .list_separator(","),
            )
            .build()?
            .try_deserialize()
            .map_err(Into::into)
    }
}
