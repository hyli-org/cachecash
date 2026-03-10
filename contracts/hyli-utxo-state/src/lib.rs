#![cfg_attr(feature = "guest", no_std)]

extern crate alloc;

pub mod state;
pub mod zk;

use alloc::vec::Vec;
pub use state::{HyliUtxoState, HyliUtxoZkVmBatch, HyliUtxoZkVmState};

use jolt_sdk as jolt;

#[jolt::provable(
    stack_size = 131_072,
    heap_size = 262_144,
    max_trace_length = 8_388_608,
    max_input_size = 10_000,
    max_output_size = 10_000
)]
fn run(commitment_metadata: Vec<u8>, calldatas: Vec<sdk::Calldata>) -> Vec<sdk::HyliOutput> {
    sdk::guest::execute::<HyliUtxoZkVmBatch>(&commitment_metadata, &calldatas)
}
