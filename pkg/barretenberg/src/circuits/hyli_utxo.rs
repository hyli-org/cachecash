use super::note::{BInputNote, BNote};
use crate::{
    Result,
    backend::DefaultBackend,
    circuits::get_bytecode_from_program,
    prove::prove,
    traits::{Prove, Verify},
    util::write_to_temp_file,
    verify::{VerificationKey, VerificationKeyHash},
};
use element::Base;
use lazy_static::lazy_static;
use noirc_abi::{InputMap, input_parser::InputValue};
use noirc_artifacts::program::ProgramArtifact;
use noirc_driver::CompiledProgram;
use std::{io::Write, path::PathBuf, process::Command};
use tempfile::NamedTempFile;
use zk_primitives::{
    HYLI_UTXO_PUBLIC_INPUTS_COUNT, HyliUtxo, HyliUtxoProof, ToBytes, UtxoProofBytes,
    bytes_to_elements,
};

const PROGRAM: &str = include_str!("../../../../fixtures/programs/hyli_utxo.json");
const KEY: &[u8] = include_bytes!("../../../../fixtures/keys/hyli_utxo_key");
const KEY_FIELDS: &[u8] = include_bytes!("../../../../fixtures/keys/hyli_utxo_key_fields.json");

lazy_static! {
    static ref PROGRAM_ARTIFACT: ProgramArtifact = serde_json::from_str(PROGRAM).unwrap();
    static ref PROGRAM_COMPILED: CompiledProgram = CompiledProgram::from(PROGRAM_ARTIFACT.clone());
    static ref PROGRAM_PATH: PathBuf = write_to_temp_file(PROGRAM.as_bytes(), ".json");
    static ref BYTECODE: Vec<u8> = get_bytecode_from_program(PROGRAM);
    pub static ref HYLI_UTXO_VERIFICATION_KEY: VerificationKey = {
        let fields = serde_json::from_slice::<Vec<Base>>(KEY_FIELDS).unwrap();
        VerificationKey(fields)
    };
    pub static ref HYLI_UTXO_VERIFICATION_KEY_HASH: VerificationKeyHash = VerificationKeyHash(
        bn254_blackbox_solver::poseidon_hash(&HYLI_UTXO_VERIFICATION_KEY.0, false).unwrap()
    );
}

impl Prove for HyliUtxo {
    type Proof = HyliUtxoProof;
    type Result<Proof> = Result<Proof>;

    fn prove(&self) -> Self::Result<Self::Proof> {
        let inputs = build_hyli_input_map(self);

        let proof_bytes = prove::<DefaultBackend>(
            &PROGRAM_COMPILED,
            PROGRAM.as_bytes(),
            &BYTECODE,
            KEY,
            &inputs,
            true,
            false,
        )?;

        let public_inputs = proof_bytes[..HYLI_UTXO_PUBLIC_INPUTS_COUNT * 32].to_vec();
        let public_inputs = bytes_to_elements(&public_inputs);
        let raw_proof = proof_bytes[HYLI_UTXO_PUBLIC_INPUTS_COUNT * 32..].to_vec();

        Ok(HyliUtxoProof {
            proof: UtxoProofBytes(raw_proof),
            public_inputs,
        })
    }
}

impl Verify for HyliUtxoProof {
    fn verify(&self) -> Result<()> {
        verify_with_bb(KEY, &self.to_bytes())
    }
}

fn build_hyli_input_map(value: &HyliUtxo) -> InputMap {
    let mut map = InputMap::new();

    map.insert(
        "version".to_owned(),
        InputValue::Field(Base::from(value.version as u64)),
    );
    map.insert(
        "initial_state_len".to_owned(),
        InputValue::Field(Base::from(value.initial_state.len() as u64)),
    );
    map.insert(
        "initial_state".to_owned(),
        InputValue::Vec(
            value
                .initial_state
                .iter()
                .map(|b| InputValue::Field(Base::from(*b as u64)))
                .collect(),
        ),
    );
    map.insert(
        "next_state_len".to_owned(),
        InputValue::Field(Base::from(value.next_state.len() as u64)),
    );
    map.insert(
        "next_state".to_owned(),
        InputValue::Vec(
            value
                .next_state
                .iter()
                .map(|b| InputValue::Field(Base::from(*b as u64)))
                .collect(),
        ),
    );
    map.insert(
        "identity_len".to_owned(),
        InputValue::Field(Base::from(value.identity_len as u64)),
    );
    map.insert(
        "identity".to_owned(),
        InputValue::String(value.padded_identity()),
    );
    map.insert(
        "tx_hash".to_owned(),
        InputValue::String(value.padded_tx_hash()),
    );
    map.insert(
        "index".to_owned(),
        InputValue::Field(Base::from(value.index as u64)),
    );
    map.insert(
        "blob_number".to_owned(),
        InputValue::Field(Base::from(value.blob_number as u64)),
    );
    map.insert(
        "blob_index".to_owned(),
        InputValue::Field(Base::from(value.blob_index as u64)),
    );
    map.insert(
        "blob_contract_name_len".to_owned(),
        InputValue::Field(Base::from(value.blob_contract_name_len as u64)),
    );
    map.insert(
        "blob_contract_name".to_owned(),
        InputValue::String(value.padded_blob_contract_name()),
    );
    map.insert(
        "blob_capacity".to_owned(),
        InputValue::Field(Base::from(value.blob_capacity as u64)),
    );
    map.insert(
        "blob_len".to_owned(),
        InputValue::Field(Base::from(value.blob_len as u64)),
    );
    map.insert(
        "blob".to_owned(),
        InputValue::Vec(
            value
                .blob
                .iter()
                .map(|b| InputValue::Field(Base::from(*b as u64)))
                .collect(),
        ),
    );
    map.insert(
        "tx_blob_count".to_owned(),
        InputValue::Field(Base::from(value.tx_blob_count as u64)),
    );
    map.insert(
        "success".to_owned(),
        InputValue::Field(Base::from(value.success as u64)),
    );

    map.insert(
        "notes_root_initial".to_owned(),
        InputValue::Field(value.notes_root_initial.to_base()),
    );
    map.insert(
        "nullifier_root_initial".to_owned(),
        InputValue::Field(value.nullifier_root_initial.to_base()),
    );

    let input_notes: [BInputNote; 2] = value
        .utxo
        .input_notes
        .iter()
        .map(BInputNote::from)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    map.insert(
        "input_notes".to_owned(),
        InputValue::Vec(input_notes.map(InputValue::from).to_vec()),
    );

    let output_notes: [BNote; 2] = value
        .utxo
        .output_notes
        .iter()
        .map(BNote::from)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    map.insert(
        "output_notes".to_owned(),
        InputValue::Vec(output_notes.map(InputValue::from).to_vec()),
    );

    let messages = value.messages();
    let pmessage4 = messages[4];
    map.insert(
        "pmessage4".to_owned(),
        InputValue::Field(pmessage4.to_base()),
    );

    let commitments = value.commitments();
    map.insert(
        "commitments".to_owned(),
        InputValue::Vec(
            commitments
                .into_iter()
                .map(|commitment| InputValue::Field(commitment.to_base()))
                .collect(),
        ),
    );

    map.insert(
        "messages".to_owned(),
        InputValue::Vec(
            messages
                .into_iter()
                .map(|message| InputValue::Field(message.to_base()))
                .collect(),
        ),
    );

    map.insert(
        "input_merkle_paths".to_owned(),
        InputValue::Vec(
            value
                .input_merkle_paths
                .iter()
                .map(|path| {
                    InputValue::Vec(
                        path.iter()
                            .map(|element| InputValue::Field(element.to_base()))
                            .collect(),
                    )
                })
                .collect(),
        ),
    );
    map.insert(
        "output_merkle_paths".to_owned(),
        InputValue::Vec(
            value
                .output_merkle_paths
                .iter()
                .map(|path| {
                    InputValue::Vec(
                        path.iter()
                            .map(|element| InputValue::Field(element.to_base()))
                            .collect(),
                    )
                })
                .collect(),
        ),
    );
    map.insert(
        "nullifier_merkle_paths".to_owned(),
        InputValue::Vec(
            value
                .nullifier_merkle_paths
                .iter()
                .map(|path| {
                    InputValue::Vec(
                        path.iter()
                            .map(|element| InputValue::Field(element.to_base()))
                            .collect(),
                    )
                })
                .collect(),
        ),
    );

    map
}

fn verify_with_bb(key: &[u8], proof_with_inputs: &[u8]) -> Result<()> {
    let mut key_file = NamedTempFile::new()?;
    key_file.write_all(key)?;
    key_file.flush()?;

    let public_inputs_len = HYLI_UTXO_PUBLIC_INPUTS_COUNT * 32;
    if proof_with_inputs.len() < public_inputs_len {
        return Err("Proof is shorter than expected".into());
    }

    let proof_fields_len = proof_with_inputs.len() - public_inputs_len;
    if proof_fields_len % 32 != 0 {
        return Err("Proof field data must be a multiple of 32 bytes".into());
    }

    let total_fields = (HYLI_UTXO_PUBLIC_INPUTS_COUNT + (proof_fields_len / 32)) as u32;

    let mut encoded = Vec::with_capacity(4 + proof_with_inputs.len());
    encoded.extend_from_slice(&total_fields.to_be_bytes());
    encoded.extend_from_slice(&proof_with_inputs[..public_inputs_len]);
    encoded.extend_from_slice(&proof_with_inputs[public_inputs_len..]);

    let mut proof_file = NamedTempFile::new()?;
    proof_file.write_all(&encoded)?;
    proof_file.flush()?;

    let mut public_inputs_file = NamedTempFile::new()?;
    public_inputs_file.write_all(&proof_with_inputs[..public_inputs_len])?;
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

    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8(output.stderr)?;
        return Err(stderr.into());
    }

    Ok(())
}
