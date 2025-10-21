pub const MERKLE_TREE_PATH_DEPTH: usize = 160;
pub const MERKLE_TREE_DEPTH: usize = 161;
pub const MAXIMUM_TXNS: usize = UTXO_AGG_NUMBER * UTXO_AGGREGATIONS;
pub const UTXO_AGGREGATIONS: usize = 2;
pub const UTXO_AGG_NUMBER: usize = 3;
pub const UTXO_INPUTS: usize = 2;
pub const UTXO_OUTPUTS: usize = 2;
pub const UTXO_AGG_LEAVES: usize = UTXO_AGG_NUMBER * (UTXO_INPUTS + UTXO_OUTPUTS);
