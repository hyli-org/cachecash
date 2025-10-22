use borsh::{BorshDeserialize, BorshSerialize};
use sdk::{
    Blob, BlobData, BlobIndex, BlobTransaction, ContractAction, ContractName, StructuredBlobData,
};
use serde::{Deserialize, Serialize};
use zk_primitives::Utxo;

#[derive(Debug, Deserialize)]
pub struct FaucetRequest {
    pub name: String,
    #[serde(default)]
    pub amount: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct KeyPairInfo {
    pub private_key_hex: String,
    pub public_key_hex: String,
}

#[derive(Debug, Serialize)]
pub struct FaucetResponse {
    pub name: String,
    pub key_pair: KeyPairInfo,
    pub contract_name: String,
    pub amount: u64,
    pub tx_hash: String,
    pub transaction: BlobTransaction,
    pub utxo: Utxo,
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
