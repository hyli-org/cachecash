use crate::{
    Result,
    backend::DefaultBackend,
    circuits::get_bytecode_from_program,
    prove::prove,
    traits::{Prove, Verify},
};
use element::Base;
use lazy_static::lazy_static;
use noirc_abi::{InputMap, input_parser::InputValue};
use noirc_artifacts::program::ProgramArtifact;
use noirc_driver::CompiledProgram;
use std::collections::BTreeMap;
use zk_primitives::{
    HYLI_SMT_INCL_BLOB_LENGTH_BYTES, HYLI_SMT_INCL_PUBLIC_INPUTS_COUNT, HyliSmtIncl,
    HyliSmtInclProof, UtxoProofBytes, bytes_to_elements,
};

const PROGRAM: &str = include_str!("../../../../fixtures/programs/hyli_smt_incl_proof.json");
const KEY: &[u8] = include_bytes!("../../../../fixtures/keys/hyli_smt_incl_proof_key");

lazy_static! {
    static ref PROGRAM_ARTIFACT: ProgramArtifact = serde_json::from_str(PROGRAM).unwrap();
    static ref PROGRAM_COMPILED: CompiledProgram = CompiledProgram::from(PROGRAM_ARTIFACT.clone());
    static ref BYTECODE: Vec<u8> = get_bytecode_from_program(PROGRAM);
}

impl Prove for HyliSmtIncl {
    type Proof = HyliSmtInclProof;
    type Result<Proof> = Result<Proof>;

    fn prove(&self) -> Self::Result<Self::Proof> {
        let inputs = build_smt_incl_input_map(self);

        let proof_bytes = prove::<DefaultBackend>(
            &PROGRAM_COMPILED,
            PROGRAM.as_bytes(),
            &BYTECODE,
            KEY,
            &inputs,
            false,
            false,
        )?;

        let public_inputs = proof_bytes[..HYLI_SMT_INCL_PUBLIC_INPUTS_COUNT * 32].to_vec();
        let public_inputs = bytes_to_elements(&public_inputs);
        let raw_proof = proof_bytes[HYLI_SMT_INCL_PUBLIC_INPUTS_COUNT * 32..].to_vec();

        Ok(HyliSmtInclProof {
            proof: UtxoProofBytes(raw_proof),
            public_inputs,
        })
    }
}

impl Verify for HyliSmtInclProof {
    fn verify(&self) -> Result<()> {
        use std::io::Write;
        use std::process::Command;
        use tempfile::NamedTempFile;
        use zk_primitives::ToBytes;

        let proof_with_inputs = self.to_bytes();
        let public_inputs_len = HYLI_SMT_INCL_PUBLIC_INPUTS_COUNT * 32;
        if proof_with_inputs.len() < public_inputs_len {
            return Err("Proof is shorter than expected".into());
        }

        let (public_inputs_bytes, proof_bytes) = proof_with_inputs.split_at(public_inputs_len);

        let mut key_file = NamedTempFile::new()?;
        key_file.write_all(KEY)?;
        key_file.flush()?;

        let mut proof_file = NamedTempFile::new()?;
        proof_file.write_all(proof_bytes)?;
        proof_file.flush()?;

        let mut public_inputs_file = NamedTempFile::new()?;
        public_inputs_file.write_all(public_inputs_bytes)?;
        public_inputs_file.flush()?;

        let output = Command::new("bb")
            .arg("verify")
            .arg("-v")
            .arg("--scheme")
            .arg("ultra_honk")
            .arg("-k")
            .arg(key_file.path())
            .arg("-p")
            .arg(proof_file.path())
            .arg("-i")
            .arg(public_inputs_file.path())
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8(output.stderr)?;
            return Err(stderr.into());
        }

        Ok(())
    }
}

fn build_smt_incl_input_map(value: &HyliSmtIncl) -> InputMap {
    let mut map = InputMap::new();
    let tx_hash_bytes = decode_tx_hash_32(&value.tx_hash);

    assert!(
        value.blob_capacity as usize == HYLI_SMT_INCL_BLOB_LENGTH_BYTES,
        "blob capacity must be {} bytes",
        HYLI_SMT_INCL_BLOB_LENGTH_BYTES
    );
    assert!(
        value.blob_len as usize == HYLI_SMT_INCL_BLOB_LENGTH_BYTES,
        "blob length must be {} bytes",
        HYLI_SMT_INCL_BLOB_LENGTH_BYTES
    );

    let mut blob = BTreeMap::new();
    blob.insert(
        "index".to_owned(),
        InputValue::Field(Base::from(value.blob_index as u64)),
    );
    blob.insert(
        "contract_name_len".to_owned(),
        InputValue::Field(Base::from(value.blob_contract_name_len as u64)),
    );
    blob.insert(
        "contract_name".to_owned(),
        InputValue::String(value.padded_blob_contract_name()),
    );
    blob.insert(
        "data_len".to_owned(),
        InputValue::Field(Base::from(value.blob_len as u64)),
    );
    blob.insert(
        "data".to_owned(),
        InputValue::Vec(
            value
                .blob
                .iter()
                .map(|b| InputValue::Field(Base::from(*b as u64)))
                .collect(),
        ),
    );

    let mut hyli_output = BTreeMap::new();
    hyli_output.insert(
        "version".to_owned(),
        InputValue::Field(Base::from(value.version as u64)),
    );
    hyli_output.insert(
        "initial_state_len".to_owned(),
        InputValue::Field(Base::from(value.initial_state.len() as u64)),
    );
    hyli_output.insert(
        "initial_state_max".to_owned(),
        InputValue::Field(Base::from(4u64)),
    );
    hyli_output.insert(
        "initial_state".to_owned(),
        InputValue::Vec(
            value
                .initial_state
                .iter()
                .map(|b| InputValue::Field(Base::from(*b as u64)))
                .collect(),
        ),
    );
    hyli_output.insert(
        "next_state_len".to_owned(),
        InputValue::Field(Base::from(value.next_state.len() as u64)),
    );
    hyli_output.insert(
        "next_state_max".to_owned(),
        InputValue::Field(Base::from(4u64)),
    );
    hyli_output.insert(
        "next_state".to_owned(),
        InputValue::Vec(
            value
                .next_state
                .iter()
                .map(|b| InputValue::Field(Base::from(*b as u64)))
                .collect(),
        ),
    );
    hyli_output.insert(
        "identity_len".to_owned(),
        InputValue::Field(Base::from(value.identity_len as u64)),
    );
    hyli_output.insert(
        "identity_max".to_owned(),
        InputValue::Field(Base::from(256u64)),
    );
    hyli_output.insert(
        "identity".to_owned(),
        InputValue::String(value.padded_identity()),
    );
    hyli_output.insert(
        "index".to_owned(),
        InputValue::Field(Base::from(value.index as u64)),
    );
    hyli_output.insert(
        "blob_count".to_owned(),
        InputValue::Field(Base::from(value.blob_number as u64)),
    );
    hyli_output.insert("blob_slots".to_owned(), InputValue::Field(Base::from(1u64)));
    hyli_output.insert(
        "blob_name_max".to_owned(),
        InputValue::Field(Base::from(256u64)),
    );
    hyli_output.insert(
        "blob_data_max".to_owned(),
        InputValue::Field(Base::from(HYLI_SMT_INCL_BLOB_LENGTH_BYTES as u64)),
    );
    hyli_output.insert(
        "blobs".to_owned(),
        InputValue::Vec(vec![InputValue::Struct(blob)]),
    );
    hyli_output.insert(
        "tx_blob_count".to_owned(),
        InputValue::Field(Base::from(value.tx_blob_count as u64)),
    );
    hyli_output.insert(
        "tx_hash".to_owned(),
        InputValue::Vec(
            tx_hash_bytes
                .iter()
                .map(|b| InputValue::Field(Base::from(*b as u64)))
                .collect(),
        ),
    );
    hyli_output.insert(
        "success".to_owned(),
        InputValue::Field(Base::from(value.success as u64)),
    );
    hyli_output.insert(
        "program_outputs_max".to_owned(),
        InputValue::Field(Base::from(5u64)),
    );
    hyli_output.insert(
        "program_outputs_len".to_owned(),
        InputValue::Field(Base::from(0u64)),
    );
    hyli_output.insert(
        "program_outputs".to_owned(),
        InputValue::Vec(
            [0u8; 5]
                .iter()
                .map(|b| InputValue::Field(Base::from(*b as u64)))
                .collect(),
        ),
    );

    map.insert("hyli_output".to_owned(), InputValue::Struct(hyli_output));

    map.insert(
        "input_notes".to_owned(),
        InputValue::Vec(
            value
                .input_notes
                .iter()
                .map(|input_note| {
                    let mut struct_ = std::collections::BTreeMap::new();
                    struct_.insert("note".to_owned(), InputValue::from(&input_note.note));
                    struct_.insert(
                        "secret_key".to_owned(),
                        InputValue::Field(input_note.secret_key.to_base()),
                    );
                    InputValue::Struct(struct_)
                })
                .collect(),
        ),
    );

    map.insert(
        "siblings_0".to_owned(),
        InputValue::Vec(
            value
                .siblings_0
                .iter()
                .map(|f| InputValue::Field(*f))
                .collect(),
        ),
    );
    map.insert(
        "siblings_1".to_owned(),
        InputValue::Vec(
            value
                .siblings_1
                .iter()
                .map(|f| InputValue::Field(*f))
                .collect(),
        ),
    );

    map
}

fn decode_tx_hash_32(value: &str) -> [u8; 32] {
    let normalized = value.trim_end_matches('\0').trim_start_matches("0x");
    assert!(
        normalized.len() == 64,
        "tx_hash must be 64 hex chars, got {}",
        normalized.len()
    );
    let bytes_slice = normalized.as_bytes();
    let mut bytes = [0u8; 32];
    for i in 0..32 {
        let hi = hex_nibble(bytes_slice[i * 2]);
        let lo = hex_nibble(bytes_slice[i * 2 + 1]);
        bytes[i] = (hi << 4) | lo;
    }
    bytes
}

fn hex_nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        b'A'..=b'F' => value - b'A' + 10,
        _ => panic!("invalid hex character in tx_hash"),
    }
}
