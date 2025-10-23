use crate::{ToBytes, Utxo, UtxoProofBytes};
use borsh::{BorshDeserialize, BorshSerialize};
use element::Element;
use hash::hash_merge;
use serde::{Deserialize, Serialize};

/// Number of public input fields emitted by the Hyli UTXO proof.
pub const HYLI_UTXO_PUBLIC_INPUTS_COUNT: usize = 733;

/// Number of field elements concatenated into the Hyli blob (2 input commitments + 2 nullifier commitments).
pub const HYLI_BLOB_HASH_COUNT: usize = 4;

/// Size in bytes of a single field element commitment within the blob.
pub const HYLI_BLOB_HASH_BYTE_LENGTH: usize = 32;

/// Total length in bytes of the Hyli blob.
pub const HYLI_BLOB_LENGTH_BYTES: usize = HYLI_BLOB_HASH_COUNT * HYLI_BLOB_HASH_BYTE_LENGTH;

/// Hyli-specific metadata and witness values required to construct the Hyli UTXO proof.
#[derive(Debug, Clone)]
pub struct HyliUtxo {
    /// Circuit version expected by Hyli.
    pub version: u32,
    /// Serialized initial state digest (notes + nullifier roots).
    pub initial_state: [u8; 4],
    /// Serialized next state digest (notes + nullifier roots).
    pub next_state: [u8; 4],
    /// Number of significant bytes in the identity string.
    pub identity_len: u8,
    /// Identity payload (will be padded to 256 bytes when proving).
    pub identity: String,
    /// Transaction hash (will be padded to 64 bytes when proving).
    pub tx_hash: String,
    /// Transaction index inside the blob.
    pub index: u32,
    /// Blob number inside the batch.
    pub blob_number: u32,
    /// Index of the blob within the Hyli transaction call.
    pub blob_index: u32,
    /// Declared length for the blob contract name.
    pub blob_contract_name_len: u8,
    /// Contract name attached to the blob (will be padded to 256 bytes).
    pub blob_contract_name: String,
    /// Blob capacity advertised by the host.
    pub blob_capacity: u32,
    /// Actual blob length.
    pub blob_len: u32,
    /// Blob payload (input commitments followed by nullifier commitments) exposed publicly.
    pub blob: [u8; HYLI_BLOB_LENGTH_BYTES],
    /// Number of blobs included in the transaction.
    pub tx_blob_count: u32,
    /// Execution success flag reported by the host.
    pub success: bool,
    /// Underlying UTXO transaction data.
    pub utxo: Utxo,
}

impl HyliUtxo {
    /// Returns the commitments referenced by this transaction.
    #[must_use]
    pub fn commitments(&self) -> [Element; 4] {
        [
            self.utxo.input_notes[0].note.commitment(),
            self.utxo.input_notes[1].note.commitment(),
            self.utxo.output_notes[0].commitment(),
            self.utxo.output_notes[1].commitment(),
        ]
    }

    /// Returns the message payload derived from the inner UTXO transaction.
    #[must_use]
    pub fn messages(&self) -> [Element; 5] {
        self.utxo.messages()
    }

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

    /// Returns the private commitments inserted into the nullifier tree for each input note.
    #[must_use]
    pub fn nullifier_commitments(&self) -> [Element; 2] {
        let mut commitments = [Element::ZERO; 2];
        for (index, note) in self.utxo.input_notes.iter().enumerate() {
            commitments[index] = hash_merge([note.note.psi, note.secret_key]);
        }
        commitments
    }

    /// Computes the expected blob payload derived from the underlying commitments.
    #[must_use]
    pub fn expected_blob(&self) -> [u8; HYLI_BLOB_LENGTH_BYTES] {
        let commitments = self.commitments();
        let nullifiers = self.nullifier_commitments();
        let mut blob = [0u8; HYLI_BLOB_LENGTH_BYTES];
        let mut write_index = 0usize;

        for field in commitments.iter().take(2) {
            blob[write_index..write_index + HYLI_BLOB_HASH_BYTE_LENGTH]
                .copy_from_slice(&field.to_be_bytes());
            write_index += HYLI_BLOB_HASH_BYTE_LENGTH;
        }

        for field in nullifiers.iter() {
            blob[write_index..write_index + HYLI_BLOB_HASH_BYTE_LENGTH]
                .copy_from_slice(&field.to_be_bytes());
            write_index += HYLI_BLOB_HASH_BYTE_LENGTH;
        }

        blob
    }
}

fn pad_string(value: &str, target_len: usize) -> String {
    assert!(
        value.len() <= target_len,
        "string '{}' exceeds maximum length {}",
        value,
        target_len
    );

    let mut padded = String::with_capacity(target_len);
    padded.push_str(value);
    if value.len() < target_len {
        padded.extend(std::iter::repeat('\0').take(target_len - value.len()));
    }
    padded
}

/// Hyli UTXO proof wrapper.
#[derive(Default, Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct HyliUtxoProof {
    /// Serialized proof bytes emitted by Barretenberg.
    pub proof: UtxoProofBytes,
    /// Public inputs exposed by the circuit.
    pub public_inputs: Vec<Element>,
}

impl ToBytes for HyliUtxoProof {
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.public_inputs.len() * 32 + self.proof.0.len());
        for element in &self.public_inputs {
            bytes.extend_from_slice(&element.to_be_bytes());
        }
        bytes.extend_from_slice(&self.proof.0);
        bytes
    }
}
