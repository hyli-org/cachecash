use crate::types::ZfruitAction;
use sdk::{BlobTransaction, ContractAction, ContractName, Identity};

pub const FAUCET_IDENTITY_PREFIX: &str = "faucet";
pub const HYLI_UTXO_CONTRACT_NAME: &str = "hyli_utxo";

pub fn build_faucet_transaction(
    _contract_name: &ContractName,
    recipient_pubkey: Vec<u8>,
    amount: u64,
) -> BlobTransaction {
    let action = ZfruitAction::Faucet {
        recipient_pubkey,
        amount,
    };

    let contract_name = ContractName(HYLI_UTXO_CONTRACT_NAME.to_string());
    let identity = Identity(format!("{}@{}", FAUCET_IDENTITY_PREFIX, contract_name.0));

    BlobTransaction::new(identity, vec![action.as_blob(contract_name, None, None)])
}
