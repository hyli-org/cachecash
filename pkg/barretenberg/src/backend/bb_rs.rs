use super::Backend;
use crate::Result;
use lazy_static::lazy_static;
use std::sync::{Mutex, Once};

pub struct BindingBackend;

lazy_static! {
    static ref INIT: Once = Once::new();
    static ref BB_MUTEX: Mutex<()> = Mutex::new(());
}

const G2: [u8; 128] = [
    0x01, 0x18, 0xC4, 0xD5, 0xB8, 0x37, 0xBC, 0xC2, 0xBC, 0x89, 0xB5, 0xB3, 0x98, 0xB5, 0x97, 0x4E,
    0x9F, 0x59, 0x44, 0x07, 0x3B, 0x32, 0x07, 0x8B, 0x7E, 0x23, 0x1F, 0xEC, 0x93, 0x88, 0x83, 0xB0,
    0x26, 0x0E, 0x01, 0xB2, 0x51, 0xF6, 0xF1, 0xC7, 0xE7, 0xFF, 0x4E, 0x58, 0x07, 0x91, 0xDE, 0xE8,
    0xEA, 0x51, 0xD8, 0x7A, 0x35, 0x8E, 0x03, 0x8B, 0x4E, 0xFE, 0x30, 0xFA, 0xC0, 0x93, 0x83, 0xC1,
    0x22, 0xFE, 0xBD, 0xA3, 0xC0, 0xC0, 0x63, 0x2A, 0x56, 0x47, 0x5B, 0x42, 0x14, 0xE5, 0x61, 0x5E,
    0x11, 0xE6, 0xDD, 0x3F, 0x96, 0xE6, 0xCE, 0xA2, 0x85, 0x4A, 0x87, 0xD4, 0xDA, 0xCC, 0x5E, 0x55,
    0x04, 0xFC, 0x63, 0x69, 0xF7, 0x11, 0x0F, 0xE3, 0xD2, 0x51, 0x56, 0xC1, 0xBB, 0x9A, 0x72, 0x85,
    0x9C, 0xF2, 0xA0, 0x46, 0x41, 0xF9, 0x9B, 0xA4, 0xEE, 0x41, 0x3C, 0x80, 0xDA, 0x6A, 0x5F, 0xE4,
];

#[cfg(feature = "bb_utxo")]
lazy_static! {
    static ref G1: &'static [u8] = include_bytes!("../../../../fixtures/params/g1.utxo.dat");
}

#[cfg(not(feature = "bb_utxo"))]
lazy_static! {
    static ref G1: &'static [u8] = include_bytes!("../../../../fixtures/params/g1.max.dat");
}

impl BindingBackend {
    fn load_srs() {
        INIT.call_once(|| unsafe {
            bb_rs::barretenberg_api::srs::init_srs(&G1, (G1.len() / 64) as u32, &G2);
        });
    }
}

impl Backend for BindingBackend {
    fn prove(
        _program: &[u8],
        bytecode: &[u8],
        key: &[u8],
        witness: &[u8],
        _recursive: bool,
        oracle_hash_keccak: bool,
    ) -> Result<Vec<u8>> {
        let _guard = BB_MUTEX.lock().unwrap();

        Self::load_srs();

        let proof = match oracle_hash_keccak {
            false => unsafe {
                bb_rs::barretenberg_api::acir::acir_prove_ultra_honk(bytecode, witness, key)
            },
            true => unsafe {
                bb_rs::barretenberg_api::acir::acir_prove_ultra_keccak_zk_honk(
                    bytecode, witness, key,
                )
            },
        };

        Ok(proof)
    }

    fn verify(proof: &[u8], key: &[u8], oracle_hash_keccak: bool) -> Result<()> {
        let _guard = BB_MUTEX.lock().unwrap();

        Self::load_srs();

        let verified = match oracle_hash_keccak {
            false => unsafe { bb_rs::barretenberg_api::acir::acir_verify_ultra_honk(proof, key) },
            true => unsafe {
                bb_rs::barretenberg_api::acir::acir_verify_ultra_keccak_zk_honk(proof, key)
            },
        };

        match verified {
            true => Ok(()),
            false => Err("Proof verification failed".to_owned().into()),
        }
    }
}
