#![no_main]

use sdk::{
    guest::{GuestEnv, SP1Env},
    Calldata,
};

sp1_zkvm::entrypoint!(main);

fn main() {
    let env = SP1Env {};
    let _: (Vec<u8>, Vec<Calldata>) = env.read();
    panic!("hyli-utxo-state guest execution is no longer supported");
}
