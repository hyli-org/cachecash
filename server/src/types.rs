use borsh::{BorshDeserialize, BorshSerialize};
use sdk::{Blob, BlobData, BlobIndex, ContractAction, ContractName, StructuredBlobData};
use serde::{Deserialize, Serialize};
use zk_primitives::Note;

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
