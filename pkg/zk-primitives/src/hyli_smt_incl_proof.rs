use crate::{InputNote, ToBytes, UtxoProofBytes};
use borsh::{BorshDeserialize, BorshSerialize};
use element::{Base, Element};
use serde::{Deserialize, Serialize};

/// Number of public input fields emitted by the Hyli SMT inclusion proof circuit.
pub const HYLI_SMT_INCL_PUBLIC_INPUTS_COUNT: usize = 691;

/// Total length in bytes of the SMT inclusion proof blob.
/// Layout: [nullifier_0 (32B)][nullifier_1 (32B)][notes_root (32B)]
pub const HYLI_SMT_INCL_BLOB_LENGTH_BYTES: usize = 96;

/// Hyli-specific metadata and witness values required to construct the SMT inclusion proof.
#[derive(Debug, Clone)]
pub struct HyliSmtIncl {
    /// Circuit version expected by Hyli.
    pub version: u32,
    /// Serialized initial state digest.
    pub initial_state: [u8; 4],
    /// Serialized next state digest.
    pub next_state: [u8; 4],
    /// Number of significant bytes in the identity string.
    pub identity_len: u8,
    /// Identity payload (padded to 256 bytes when proving).
    pub identity: String,
    /// Transaction hash (padded to 64 bytes when proving).
    pub tx_hash: String,
    /// Transaction index inside the blob.
    pub index: u32,
    /// Blob number inside the batch.
    pub blob_number: u32,
    /// Index of the blob within the Hyli transaction call.
    pub blob_index: u32,
    /// Declared length for the blob contract name.
    pub blob_contract_name_len: u8,
    /// Contract name attached to the blob (padded to 256 bytes).
    pub blob_contract_name: String,
    /// Blob capacity advertised by the host (must be 96).
    pub blob_capacity: u32,
    /// Actual blob length (must be 96).
    pub blob_len: u32,
    /// 96-byte blob: [nullifier_0 (32B)][nullifier_1 (32B)][notes_root (32B)]
    pub blob: [u8; HYLI_SMT_INCL_BLOB_LENGTH_BYTES],
    /// Number of blobs included in the transaction.
    pub tx_blob_count: u32,
    /// Execution success flag reported by the host.
    pub success: bool,
    /// Input notes (note data + secret key) whose commitments are proven to be in the SMT.
    pub input_notes: [InputNote; 2],
    /// SMT siblings for input_notes[0] commitment (256 Field elements).
    pub siblings_0: Box<[Base; 256]>,
    /// SMT siblings for input_notes[1] commitment (256 Field elements).
    pub siblings_1: Box<[Base; 256]>,
}

impl HyliSmtIncl {
    /// Hyli requires identity to be encoded as a fixed 256-character string.
    #[must_use]
    pub fn padded_identity(&self) -> String {
        pad_string(&self.identity, 256)
    }

    /// Hyli requires tx hash to be encoded as a fixed 64-character string.
    #[must_use]
    pub fn padded_tx_hash(&self) -> String {
        pad_string(&self.tx_hash, 64)
    }

    /// Hyli expects the blob contract name to occupy 256 characters.
    #[must_use]
    pub fn padded_blob_contract_name(&self) -> String {
        pad_string(&self.blob_contract_name, 256)
    }
}

fn pad_string(value: &str, target_len: usize) -> String {
    assert!(
        value.len() <= target_len,
        "string '{value}' exceeds maximum length {target_len}"
    );
    let mut padded = String::with_capacity(target_len);
    padded.push_str(value);
    if value.len() < target_len {
        padded.extend(std::iter::repeat_n('\0', target_len - value.len()));
    }
    padded
}

/// Hyli SMT inclusion proof wrapper.
#[derive(Default, Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct HyliSmtInclProof {
    /// Serialized proof bytes emitted by Barretenberg.
    pub proof: UtxoProofBytes,
    /// Public inputs exposed by the circuit.
    pub public_inputs: Vec<Element>,
}

impl ToBytes for HyliSmtInclProof {
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.public_inputs.len() * 32 + self.proof.0.len());
        for element in &self.public_inputs {
            bytes.extend_from_slice(&element.to_be_bytes());
        }
        bytes.extend_from_slice(&self.proof.0);
        bytes
    }
}
