use crate::{
    Result,
    backend::DefaultBackend,
    circuits::get_bytecode_from_program,
    prove::prove,
    traits::{Prove, Verify},
    util::write_to_temp_file,
    verify::{VerificationKey, VerificationKeyHash, verify},
};
use element::Base;
use lazy_static::lazy_static;
use noirc_abi::{InputMap, input_parser::InputValue};
use noirc_artifacts::program::ProgramArtifact;
use noirc_driver::CompiledProgram;
use std::path::PathBuf;
use zk_primitives::{
    Migrate, MigrateProof, MigrateProofBytes, MigratePublicInput, ToBytes, bytes_to_elements,
};

const PROGRAM: &str = include_str!("../../../../fixtures/programs/migrate.json");
const KEY: &[u8] = include_bytes!("../../../../fixtures/keys/migrate_key/vk");
const KEY_FIELDS: &[u8] = include_bytes!("../../../../fixtures/keys/migrate_key_fields.json");

lazy_static! {
    static ref PROGRAM_ARTIFACT: ProgramArtifact = serde_json::from_str(PROGRAM).unwrap();
    static ref PROGRAM_COMPILED: CompiledProgram = CompiledProgram::from(PROGRAM_ARTIFACT.clone());
    static ref PROGRAM_PATH: PathBuf = write_to_temp_file(PROGRAM.as_bytes(), ".json");
    static ref BYTECODE: Vec<u8> = get_bytecode_from_program(PROGRAM);
    pub static ref MIGRATE_VERIFICATION_KEY: VerificationKey = {
        let fields = serde_json::from_slice::<Vec<Base>>(KEY_FIELDS).unwrap();
        VerificationKey(fields)
    };
    pub static ref MIGRATE_VERIFICATION_KEY_HASH: VerificationKeyHash = VerificationKeyHash(
        bn254_blackbox_solver::poseidon_hash(&MIGRATE_VERIFICATION_KEY.0, false).unwrap()
    );
}

/// Internal struct for converting to Noir InputMap
struct MigrateInput {
    owner_pk: Base,
    old_address: Base,
    new_address: Base,
}

impl From<&Migrate> for MigrateInput {
    fn from(migrate: &Migrate) -> Self {
        MigrateInput {
            owner_pk: migrate.owner_pk.to_base(),
            old_address: migrate.old_address.to_base(),
            new_address: migrate.new_address.to_base(),
        }
    }
}

impl From<MigrateInput> for InputMap {
    fn from(input: MigrateInput) -> Self {
        let mut map = InputMap::new();
        map.insert("owner_pk".to_string(), InputValue::Field(input.owner_pk));
        map.insert(
            "old_address".to_string(),
            InputValue::Field(input.old_address),
        );
        map.insert(
            "new_address".to_string(),
            InputValue::Field(input.new_address),
        );
        map
    }
}

impl Prove for Migrate {
    type Proof = MigrateProof;
    type Result<Proof> = Result<Proof>;

    fn prove(&self) -> Self::Result<Self::Proof> {
        let inputs = InputMap::from(MigrateInput::from(self));

        let proof_bytes = prove::<DefaultBackend>(
            &PROGRAM_COMPILED,
            PROGRAM.as_bytes(),
            &BYTECODE,
            KEY,
            &inputs,
            false,
            false,
        )?;

        // The migrate circuit has 2 public inputs (old_address, new_address)
        let public_inputs_count = 2;
        let public_inputs_bytes = proof_bytes[..public_inputs_count * 32].to_vec();
        let raw_proof = proof_bytes[public_inputs_count * 32..].to_vec();

        // Parse the public inputs from bytes
        let public_inputs = bytes_to_elements(&public_inputs_bytes);
        let old_address = public_inputs[0];
        let new_address = public_inputs[1];

        Ok(MigrateProof {
            proof: MigrateProofBytes(raw_proof),
            public_inputs: MigratePublicInput {
                old_address,
                new_address,
            },
        })
    }
}

impl Verify for MigrateProof {
    fn verify(&self) -> Result<()> {
        verify::<DefaultBackend>(KEY, &self.to_bytes(), false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use element::Element;
    use zk_primitives::get_address_for_private_key;

    #[test]
    fn test_migrate_prove_and_verify() {
        // Create test values
        let owner_pk = Element::from(101u64);

        let new_address = get_address_for_private_key(owner_pk);
        let old_address = hash_poseidon::hash_merge([owner_pk, Element::ZERO]);

        let migrate = Migrate {
            owner_pk,
            old_address,
            new_address,
        };

        // Generate proof
        let proof = migrate.prove().expect("Failed to generate migrate proof");

        // Verify proof
        proof.verify().expect("Failed to verify migrate proof");
    }
}
