use borsh::{BorshDeserialize, BorshSerialize};
use sdk::{Blob, BlobData, BlobIndex, ContractAction, ContractName, StructuredBlobData};
use serde::{Deserialize, Serialize};
use zk_primitives::Note;

// ---- Address Registry API Types ----

/// Request to register a username -> UTXO address mapping.
#[derive(Debug, Deserialize)]
pub struct RegisterAddressRequest {
    /// The username (e.g., "matteo" - without @wallet suffix)
    pub username: String,
    /// The UTXO address (64-char hex, derived from poseidon2([secret_key, 0]))
    pub utxo_address: String,
    /// The secp256k1 public key for ECDH encryption (64-char hex, x-coordinate)
    pub encryption_pubkey: String,
}

/// Response after successfully registering an address.
#[derive(Debug, Serialize)]
pub struct RegisterAddressResponse {
    /// The normalized username (lowercase)
    pub username: String,
    /// The registered UTXO address
    pub utxo_address: String,
    /// The secp256k1 public key for ECDH encryption
    #[serde(skip_serializing_if = "String::is_empty")]
    pub encryption_pubkey: String,
    /// Unix timestamp when registered
    pub registered_at: u64,
    /// Whether this was an update to an existing registration
    pub was_update: bool,
}

/// Response when resolving a username to an address.
#[derive(Debug, Serialize)]
pub struct ResolveAddressResponse {
    /// The username
    pub username: String,
    /// The UTXO address
    pub utxo_address: String,
    /// The secp256k1 public key for ECDH encryption (may be empty for legacy registrations)
    #[serde(skip_serializing_if = "String::is_empty")]
    pub encryption_pubkey: String,
    /// Unix timestamp when registered
    pub registered_at: u64,
}

// ---- Encrypted Notes API Types ----

/// Request to upload an encrypted note.
#[derive(Debug, Deserialize)]
pub struct UploadNoteRequest {
    /// Recipient tag (hex-encoded, derived from recipient's public key).
    pub recipient_tag: String,
    /// Base64-encoded encrypted payload.
    pub encrypted_payload: String,
    /// Hex-encoded ephemeral public key for ECDH decryption.
    pub ephemeral_pubkey: String,
    /// Optional sender tag for grouping/filtering.
    #[serde(default)]
    pub sender_tag: Option<String>,
}

/// Response after successfully uploading a note.
#[derive(Debug, Serialize)]
pub struct UploadNoteResponse {
    /// Unique identifier for the stored note.
    pub id: String,
    /// Unix timestamp when the note was stored.
    pub stored_at: u64,
}

/// Query parameters for fetching notes.
#[derive(Debug, Deserialize, Default)]
pub struct GetNotesQuery {
    /// Only return notes stored after this Unix timestamp.
    #[serde(default)]
    pub since: Option<u64>,
    /// Maximum number of notes to return.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// A single encrypted note record in API responses.
#[derive(Debug, Serialize)]
pub struct EncryptedNoteRecord {
    /// Unique identifier.
    pub id: String,
    /// Base64-encoded encrypted payload.
    pub encrypted_payload: String,
    /// Hex-encoded ephemeral public key.
    pub ephemeral_pubkey: String,
    /// Optional sender tag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_tag: Option<String>,
    /// Unix timestamp when stored.
    pub stored_at: u64,
}

/// Response containing fetched notes.
#[derive(Debug, Serialize)]
pub struct GetNotesResponse {
    /// The encrypted notes.
    pub notes: Vec<EncryptedNoteRecord>,
    /// Whether there are more notes available beyond the limit.
    pub has_more: bool,
}

// ---- Transfer API Types ----

/// Input note data for transfer requests (includes full note + secret key)
#[derive(Debug, Clone, Deserialize)]
pub struct InputNoteData {
    /// The note being spent
    pub note: Note,
    /// Secret key for spending this note (64-char hex)
    pub secret_key: String,
}

/// Request to transfer funds between users
#[derive(Debug, Deserialize)]
pub struct TransferRequest {
    /// Recipient's public key (hex-encoded, 32 bytes)
    pub recipient_pubkey: String,
    /// Amount being transferred
    pub amount: u64,
    /// Full input notes with secret keys
    pub input_notes: [InputNoteData; 2],
    /// Output notes: [recipient_note, change_note]
    pub output_notes: [Note; 2],
}

/// Response after successful transfer
#[derive(Debug, Serialize)]
pub struct TransferResponse {
    /// Transaction hash
    pub tx_hash: String,
    /// Change note if any
    pub change_note: Option<Note>,
}

/// Request to transfer with a pre-generated proof (client-side proving)
#[derive(Debug, Deserialize)]
pub struct ProvedTransferRequest {
    /// Base64-encoded proof bytes (raw proof without public inputs)
    pub proof: String,
    /// Public inputs as hex strings (733 field elements)
    pub public_inputs: Vec<String>,
    /// 128-byte blob data
    pub blob_data: Vec<u8>,
    /// Output notes: [recipient_note, change_note]
    pub output_notes: [Note; 2],
}

// ---- Two-Step Transfer API Types ----

/// Request to create a blob transaction (step 1 of two-step transfer)
#[derive(Debug, Deserialize)]
pub struct CreateBlobRequest {
    /// 128-byte blob data: [input_commit_0, input_commit_1, nullifier_0, nullifier_1]
    pub blob_data: Vec<u8>,
    /// Output notes: [recipient_note, change_note]
    pub output_notes: [Note; 2],
}

/// Response after creating a blob transaction
#[derive(Debug, Serialize)]
pub struct CreateBlobResponse {
    /// Transaction hash from the blockchain
    pub tx_hash: String,
    /// The blobs that were included in the transaction (for client to use in proof)
    pub blobs: Vec<BlobInfo>,
}

/// Information about a blob in the transaction
#[derive(Debug, Serialize, Clone)]
pub struct BlobInfo {
    /// Contract name this blob targets
    pub contract_name: String,
    /// Blob data as hex string
    pub data: String,
}

/// Request to submit a proof for an existing blob transaction (step 2 of two-step transfer)
#[derive(Debug, Deserialize)]
pub struct SubmitProofRequest {
    /// Transaction hash from CreateBlobResponse
    pub tx_hash: String,
    /// Base64-encoded proof bytes (raw proof without public inputs)
    pub proof: String,
    /// Public inputs as hex strings (733 field elements)
    pub public_inputs: Vec<String>,
}

// ---- Existing Types ----

#[derive(Debug, Deserialize)]
pub struct FaucetRequest {
    pub pubkey_hex: String,
    #[serde(default)]
    pub amount: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct FaucetResponse {
    pub note: Note,
}

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub enum ZfruitAction {
    Faucet {
        recipient_pubkey: Vec<u8>,
        amount: u64,
    },
}

impl ContractAction for ZfruitAction {
    fn as_blob(
        &self,
        contract_name: ContractName,
        caller: Option<BlobIndex>,
        callees: Option<Vec<BlobIndex>>,
    ) -> Blob {
        Blob {
            contract_name,
            data: BlobData::from(StructuredBlobData {
                caller,
                callees,
                parameters: self.clone(),
            }),
        }
    }
}
