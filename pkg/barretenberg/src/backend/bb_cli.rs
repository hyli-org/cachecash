use std::{
    io::{Read, Write},
    path::PathBuf,
    process::Command,
};

use flate2::{Compression, read::GzEncoder};
use tempfile::{NamedTempFile, TempDir};
use tracing::{error, info};

use super::Backend;
use crate::Result;
use zk_primitives::UTXO_PUBLIC_INPUTS_COUNT;

pub struct CliBackend;

impl Backend for CliBackend {
    fn prove(
        program: &[u8],
        _bytecode: &[u8],
        _key: &[u8],
        witness: &[u8],
        recursive: bool,
        oracle_hash_keccak: bool,
    ) -> Result<Vec<u8>> {
        let mut witness_gz = GzEncoder::new(witness, Compression::none());
        let mut witness_gz_buf = Vec::with_capacity(witness.len() + 0xFF);
        witness_gz.read_to_end(&mut witness_gz_buf)?;
        let witness_gz = witness_gz_buf;

        let mut program_file = NamedTempFile::with_suffix(".json")?;
        program_file.write_all(program)?;
        program_file.flush()?;

        let mut witness_file = NamedTempFile::new()?;
        witness_file.write_all(&witness_gz)?;
        witness_file.flush()?;

        let output_dir = TempDir::new()?;

        let mut cmd = Command::new(PathBuf::from("bb"));
        cmd.arg("prove")
            .arg("-v")
            .arg("--scheme")
            .arg("ultra_honk")
            .arg("-b")
            .arg(program_file.path())
            .arg("-w")
            .arg(witness_file.path())
            .arg("-o")
            .arg(output_dir.path());

        if recursive {
            cmd.arg("--honk_recursion")
                .arg("1")
                .arg("--init_kzg_accumulator");
        }

        if oracle_hash_keccak {
            cmd.arg("--oracle_hash").arg("keccak");
        }

        let output = cmd.output()?;
        if !output.status.success() {
            let stderr = String::from_utf8(output.stderr)?;
            return Err(stderr.into());
        }

        let proof_path = output_dir.path().join("proof");
        let mut proof = std::fs::read(&proof_path)?;

        let public_inputs_path = output_dir.path().join("public_inputs");
        let public_inputs = std::fs::read(&public_inputs_path)?;

        proof.splice(0..0, public_inputs);

        Ok(proof)
    }

    fn verify(proof: &[u8], key: &[u8], oracle_hash_keccak: bool) -> Result<()> {
        let mut key_file = NamedTempFile::new()?;
        key_file.write_all(key)?;
        key_file.flush()?;

        let public_inputs_len = UTXO_PUBLIC_INPUTS_COUNT * 32;
        if proof.len() < public_inputs_len {
            return Err("Proof is shorter than expected".into());
        }

        let (public_inputs_bytes, proof_bytes) = proof.split_at(public_inputs_len);
        if proof_bytes.len() % 32 != 0 {
            return Err("Proof field data must be a multiple of 32 bytes".into());
        }

        let mut proof_file = NamedTempFile::new()?;
        proof_file.write_all(proof_bytes)?;
        proof_file.flush()?;

        let mut public_inputs_file = NamedTempFile::new()?;
        public_inputs_file.write_all(public_inputs_bytes)?;
        public_inputs_file.flush()?;

        let mut cmd = Command::new(PathBuf::from("bb"));
        cmd.arg("verify")
            .arg("-v")
            .arg("--scheme")
            .arg("ultra_honk")
            .arg("-k")
            .arg(key_file.path())
            .arg("-p")
            .arg(proof_file.path())
            .arg("-i")
            .arg(public_inputs_file.path());

        if oracle_hash_keccak {
            cmd.arg("--oracle_hash").arg("keccak");
        }

        let output = cmd.output()?;
        info!("output {:?}", output);

        if !output.status.success() {
            // TODO: return false instead? maybe pass -v and parse out verified: {0/1}
            let stderr = String::from_utf8(output.stderr)?;
            error!("proof error: {}", stderr);
            return Err(stderr.into());
        }

        Ok(())
    }
}
