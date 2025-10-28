use borsh::{BorshDeserialize, BorshSerialize};
use sdk::{Blob, BlobData, BlobIndex, ContractAction, ContractName, StructuredBlobData};
use serde::{Deserialize, Serialize};
use zk_primitives::Note;

#[derive(Debug, Deserialize)]
pub struct FaucetRequest {
    pub name: String,
    #[serde(default)]
    pub amount: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct FaucetResponse {
    pub note: Note,
}

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub enum CachecashAction {
    Faucet {
        recipient_pubkey: Vec<u8>,
        amount: u64,
    },
}

impl ContractAction for CachecashAction {
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
