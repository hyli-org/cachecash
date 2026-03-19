use client_sdk::helpers::jolt::JoltRegistryEntry;
use contracts::ELF;
use sdk::ProgramId;

pub fn load_utxo_state_registry_entry() -> (ProgramId, JoltRegistryEntry) {
    let memory_config = hyli_utxo_state::memory_config_run();

    let mut program = jolt_sdk::guest::program::Program::new(ELF, &memory_config);

    let shared = hyli_utxo_state::preprocess_shared_run(&mut program);

    let prover_preprocessing = jolt_sdk::host_utils::JoltProverPreprocessing::new(shared);

    let verifier = hyli_utxo_state::verifier_preprocessing_from_prover_run(&prover_preprocessing);

    let program_id = client_sdk::helpers::jolt::verifier_preprocessing_to_program_id(&verifier)
        .expect("Program ID should be derived from verifier preprocessing");

    (
        program_id,
        JoltRegistryEntry::new(prover_preprocessing, memory_config, ELF.to_vec()),
    )
}
