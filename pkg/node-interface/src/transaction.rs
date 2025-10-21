use element::Element;
use primitives::block_height::BlockHeight;
use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-rs")]
use ts_rs::TS;
use zk_primitives::UtxoProof;

/// Request for submit transaction
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-rs", derive(TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
pub struct TransactionRequest {
    /// Utxo proof to be verified and applied
    pub proof: UtxoProof,
}

/// Response for submit transaction
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-rs", derive(TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
pub struct TransactionResponse {
    /// Height of the block the transaction was included in
    pub height: BlockHeight,
    /// Root hash of the merkle tree for the block
    pub root_hash: Element,
    /// Transaction hash of submitted transaction
    pub txn_hash: Element,
}
