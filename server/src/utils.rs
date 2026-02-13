use std::{fs, path::Path};

use anyhow::Result;
use contracts::HYLI_UTXO_STATE_ELF;
use sp1_sdk::{Prover, ProverClient, SP1ProvingKey};
use tracing::{error, info};

pub fn load_utxo_state_proving_key(data_directory: &Path) -> Result<SP1ProvingKey> {
    let pk_path = data_directory.join("hyli_utxo_state_pk.bin");

    if pk_path.exists() {
        info!(path = %pk_path.display(), "loading SP1 proving key from disk");
        let bytes = fs::read(&pk_path)?;
        return bincode::deserialize(&bytes).map_err(Into::into);
    }

    if let Err(err) = fs::create_dir_all(data_directory) {
        error!(error = %err, "failed to create data directory for proving key");
    }

    info!(path = %pk_path.display(), "building SP1 proving key");
    let client = ProverClient::builder().cpu().build();
    let (pk, _) = client.setup(HYLI_UTXO_STATE_ELF);

    info!(path = %pk_path.display(), "persisting SP1 proving key to disk");
    if let Err(err) = fs::write(&pk_path, bincode::serialize(&pk)?) {
        error!(error = %err, "failed to persist proving key to disk");
    }

    Ok(pk)
}
