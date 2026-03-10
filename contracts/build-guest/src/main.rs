use hyli_utxo_state as guest;
use tracing::{error, info};

use jolt_sdk::{
    host_utils, JoltProof, JoltProverPreprocessing, JoltVerifierPreprocessing, Serializable,
};

fn main() {
    tracing_subscriber::fmt::init();

    // blake2b-rs has a C build step; set the RISC-V cross-compiler from the
    // standard zeroos installation so this works on any machine without
    // requiring machine-specific entries in .cargo/config.toml.
    let home = std::env::var("HOME").expect("HOME not set");
    let zeroos_cc = format!("{home}/.zeroos/musl/bin/riscv64-linux-musl-gcc");
    unsafe {
        std::env::set_var("CC_riscv64imac_zero_linux_musl", &zeroos_cc);
        std::env::set_var(
            "CFLAGS_riscv64imac_zero_linux_musl",
            "-mcmodel=medany -march=rv64imac -mabi=lp64",
        );
    }

    let args: Vec<String> = std::env::args().collect();

    // Check for --analyze [path] option
    if let Some(pos) = args.iter().position(|a| a == "--analyze") {
        let default_path = "latest_proving_data.bin".to_string();
        let path = args.get(pos + 1).unwrap_or(&default_path);
        analyze(path);
        return;
    }

    let target_dir = "target";
    info!(
        "🚀 Building the guest program in release mode {}",
        target_dir
    );
    let mut program = guest::compile_run(&target_dir);
    let elf = program.get_elf_contents();

    if let Some(elf) = elf {
        std::fs::write("./elf/elf.dat", elf).expect("Failed to write ELF file");
    } else {
        panic!("Failed to compile the guest program");
    }

    info!("✅ Program compiled successfully");
}

fn analyze(path: &str) {
    info!("🔍 Reading proving data from {}", path);
    let data = std::fs::read(path).expect("Failed to read proving data file");
    let (m, v): (Vec<u8>, Vec<sdk::Calldata>) =
        borsh::from_slice(&data).expect("Failed to deserialize proving data");
    info!("✅ Deserialized proving data, running analyze_run");

    let summary = guest::analyze_run(m.clone(), v.clone()).analyze::<jolt_sdk::F>();

    info!("📊 Proving data analysis summary:\n{:?}", summary);

    info!("🚀 Building Jolt guest program");
    let target_dir = "/tmp/jolt-guest-targets";
    let mut program = guest::compile_run(target_dir);

    info!("✅ Program compiled successfully, starting preprocessing");
    let shared_preprocessing = guest::preprocess_shared_run(&mut program);

    jolt_sdk::serialize_and_print_size(
        "shared preprocessing",
        &format!("{target_dir}/shared_preprocessing.dat"),
        &shared_preprocessing,
    )
    .expect("Failed to serialize and print size of shared preprocessing");

    info!("⚙️  Shared preprocessing completed, starting prover and verifier preprocessing");
    let prover_preprocessing = guest::preprocess_prover_run(shared_preprocessing.clone());

    info!("⚙️  Prover preprocessing built, starting verifier preprocessing");
    let verifier_setup = prover_preprocessing.generators.to_verifier_setup();
    let verifier_preprocessing =
        guest::preprocess_verifier_run(shared_preprocessing, verifier_setup, None);

    host_utils::serialize_and_print_size(
        "verifier preprocessing",
        &format!("{target_dir}/verifier_preprocessing.dat"),
        &verifier_preprocessing,
    )
    .expect("Failed to serialize and print size of verifier preprocessing");

    // let compact = jolt_sdk::JoltCompactVerifierPreprocessing::from(&verifier_preprocessing);
    // host_utils::serialize_and_print_size(
    //     "compact verifier preprocessing",
    //     &format!("{target_dir}/compact_verifier_preprocessing.dat"),
    //     &compact,
    // )
    // .expect("Failed to serialize and print size of compact verifier preprocessing");

    info!("🔐 Verifier preprocessing saved, starting proof generation");
    let prove_run = guest::build_prover_run(program, prover_preprocessing);

    info!("🔍 Building verifier run");
    let verify_run = guest::build_verifier_run(verifier_preprocessing);

    info!("⏳ Generating proof...");
    let comitment_metadata = m;
    let calldatas = v;
    let (output, proof, io_device) = prove_run(comitment_metadata.clone(), calldatas.clone());

    host_utils::serialize_and_print_size(
        "proof",
        &format!("{target_dir}/proof.dat"),
        &proof
            .serialize_to_bytes()
            .expect("Failed to serialize proof to bytes"),
    )
    .expect("Failed to serialize and print size of proof");

    info!("🔎 Proof generated, starting verification");
    let is_valid = verify_run(
        comitment_metadata,
        calldatas,
        output.clone(),
        io_device.panic,
        proof,
    );

    info!("📤 HyliOutput: {output:?}");
    if is_valid {
        info!("✅ Proof is valid!");
    } else {
        info!("❌ Proof is INVALID!");
    }

    let output = &output[0];

    let program_output = String::from_utf8(output.program_outputs.clone())
        .expect("Failed to convert output bytes to string");

    if output.success {
        info!("🎉 Program executed successfully! Output: {program_output}");
    } else {
        error!("⚠️ Program execution failed. Output: {program_output}");
    }
}
