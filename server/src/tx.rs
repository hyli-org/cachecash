use crate::types::ZfruitAction;
use sdk::{BlobTransaction, ContractAction, ContractName, Identity};

pub const FAUCET_IDENTITY_PREFIX: &str = "faucet";

pub fn build_faucet_transaction(
    contract_name: &ContractName,
    recipient_pubkey: Vec<u8>,
    amount: u64,
) -> BlobTransaction {
    let action = ZfruitAction::Faucet {
        recipient_pubkey,
        amount,
    };

    let identity = Identity(format!("{}@{}", FAUCET_IDENTITY_PREFIX, contract_name.0));
    BlobTransaction::new(
        identity,
        vec![action.as_blob(contract_name.clone(), None, None)],
    )
}
