use borsh::{BorshDeserialize, BorshSerialize};
use sdk::{Blob, BlobData, BlobIndex, ContractAction, ContractName, StructuredBlobData};
use serde::{Deserialize, Serialize};
use zk_primitives::Note;

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

/// Request to transfer funds between users
#[derive(Debug, Deserialize)]
pub struct TransferRequest {
    /// Recipient's public key (hex-encoded, 32 bytes)
    pub recipient_pubkey: String,
    /// Amount being transferred
    pub amount: u64,
    /// Output notes: [recipient_note, change_note]
    pub output_notes: [Note; 2],
    /// Input commitments (32 bytes each, hex-encoded)
    pub input_commitments: [String; 2],
    /// Nullifiers for the input notes (32 bytes each, hex-encoded)
    pub nullifiers: [String; 2],
}

/// Response after successful transfer
#[derive(Debug, Serialize)]
pub struct TransferResponse {
    /// Transaction hash
    pub tx_hash: String,
    /// Change note if any
    pub change_note: Option<Note>,
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
