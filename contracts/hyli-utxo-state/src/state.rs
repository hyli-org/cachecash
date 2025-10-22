use std::collections::HashSet;

use borsh::{BorshDeserialize, BorshSerialize};
use sdk::{utils::parse_raw_calldata, Calldata, RunResult, StateCommitment};
use sparse_merkle_tree::H256;

use crate::zk::{
    smt::{BorshableH256, SMT},
    ZkVmWitnessVec,
};

#[derive(Debug, Default)]
pub struct HyliUtxoState {
    notes_tree: SMT<BorshableH256>,
    nullified_tree: SMT<BorshableH256>,
}

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, Default)]
pub struct HyliUtxoZkVmState {
    pub notes: ZkVmWitnessVec<BorshableH256>,
    pub nullified_notes: ZkVmWitnessVec<BorshableH256>,
}

#[derive(BorshSerialize)]
struct CommitmentSnapshot {
    notes_root: BorshableH256,
    nullified_notes_root: BorshableH256,
}

#[derive(BorshSerialize, BorshDeserialize)]
struct EmptyAction;

impl HyliUtxoState {
    fn collect_leaves(tree: &SMT<BorshableH256>) -> Vec<BorshableH256> {
        let mut leaves: Vec<BorshableH256> = tree
            .store()
            .leaves_map()
            .iter()
            .map(|(key, _)| BorshableH256(*key))
            .collect();

        leaves.sort();
        leaves
    }

    pub fn record_created(&mut self, commitments: &[BorshableH256]) -> Result<(), String> {
        let mut seen = HashSet::new();
        for commitment in commitments {
            if commitment.0 == H256::zero() {
                continue;
            }
            if !seen.insert(*commitment) {
                return Err("duplicate created commitment in blob".to_string());
            }

            if self.notes_tree.contains(commitment) {
                return Err("created note already exists in notes tree".to_string());
            }

            if self.nullified_tree.contains(commitment) {
                return Err("created note already nullified".to_string());
            }

            self.notes_tree
                .update_leaf(*commitment, *commitment)
                .map_err(|e| format!("failed to insert note into SMT: {e}"))?;
        }
        Ok(())
    }

    pub fn record_nullified(&mut self, commitments: &[BorshableH256]) -> Result<(), String> {
        let mut seen = HashSet::new();
        for commitment in commitments {
            if commitment.0 == H256::zero() {
                continue;
            }
            if !seen.insert(*commitment) {
                return Err("duplicate nullified commitment in blob".to_string());
            }

            if !self.notes_tree.contains(commitment) {
                return Err("nullified note is missing from notes tree".to_string());
            }

            if self.nullified_tree.contains(commitment) {
                return Err("note has already been nullified".to_string());
            }

            self.nullified_tree
                .update_leaf(*commitment, *commitment)
                .map_err(|e| format!("failed to insert nullified note: {e}"))?;
        }
        Ok(())
    }

    pub fn notes_root(&self) -> BorshableH256 {
        self.notes_tree.root()
    }

    pub fn nullified_root(&self) -> BorshableH256 {
        self.nullified_tree.root()
    }

    pub fn to_zkvm_state(&self) -> HyliUtxoZkVmState {
        let mut notes = ZkVmWitnessVec::with_root(self.notes_root());
        for leaf in Self::collect_leaves(&self.notes_tree) {
            notes.insert(leaf);
        }

        let mut nullified = ZkVmWitnessVec::with_root(self.nullified_root());
        for leaf in Self::collect_leaves(&self.nullified_tree) {
            nullified.insert(leaf);
        }

        HyliUtxoZkVmState {
            notes,
            nullified_notes: nullified,
        }
    }
}

impl HyliUtxoZkVmState {
    fn apply_nullified(&mut self, commitments: &[BorshableH256]) -> Result<(), String> {
        let mut seen = HashSet::new();
        for commitment in commitments {
            if commitment.0 == H256::zero() {
                return Err("nullified commitment must be non-zero".to_string());
            }
            if !seen.insert(*commitment) {
                return Err("duplicate nullified commitment in blob".to_string());
            }
        }
        self.nullified_notes.values = commitments.iter().copied().collect();
        Ok(())
    }

    fn apply_created(&mut self, commitments: &[BorshableH256]) -> Result<(), String> {
        let mut seen = HashSet::new();
        for commitment in commitments {
            if commitment.0 == H256::zero() {
                return Err("created commitment must be non-zero".to_string());
            }
            if !seen.insert(*commitment) {
                return Err("duplicate created commitment in blob".to_string());
            }
        }
        self.notes.values = commitments.iter().copied().collect();
        Ok(())
    }
}

impl sdk::FullStateRevert for HyliUtxoZkVmState {}

impl sdk::ZkContract for HyliUtxoZkVmState {
    fn execute(&mut self, calldata: &Calldata) -> RunResult {
        let (_action, ctx) = parse_raw_calldata::<EmptyAction>(calldata)?;

        let hyli_blob = hyli_utxo_blob(calldata)?;
        let (nullified, created) = parse_hyli_utxo_blob(hyli_blob)?;

        self.notes.ensure_all_zero()?;
        self.nullified_notes.ensure_all_zero()?;

        self.apply_nullified(&nullified)?;
        self.apply_created(&created)?;

        Ok((Vec::new(), ctx, Vec::new()))
    }

    fn commit(&self) -> StateCommitment {
        let notes_root = self
            .notes
            .compute_root()
            .expect("compute notes root from witness");
        let nullified_root = self
            .nullified_notes
            .compute_root()
            .expect("compute nullified root from witness");

        let snapshot = CommitmentSnapshot {
            notes_root,
            nullified_notes_root: nullified_root,
        };

        StateCommitment(
            borsh::to_vec(&snapshot).expect("state commitment serialization must succeed"),
        )
    }
}

pub fn hyli_utxo_blob<'a>(calldata: &'a Calldata) -> Result<&'a [u8], String> {
    calldata
        .blobs
        .iter()
        .find(|(_, blob)| blob.contract_name.0 == "hyli_utxo")
        .map(|(_, blob)| blob.data.0.as_slice())
        .ok_or_else(|| "hyli_utxo blob not provided in calldata".to_string())
}

pub fn parse_hyli_utxo_blob(
    bytes: &[u8],
) -> Result<(Vec<BorshableH256>, Vec<BorshableH256>), String> {
    const EXPECTED_SIZE: usize = 128;
    if bytes.len() != EXPECTED_SIZE {
        return Err(format!(
            "hyli_utxo blob must be {EXPECTED_SIZE} bytes, found {}",
            bytes.len()
        ));
    }

    let commitments: Vec<BorshableH256> = bytes
        .chunks_exact(32)
        .map(|chunk| {
            <[u8; 32]>::try_from(chunk)
                .map(BorshableH256::from)
                .map_err(|_| "Failed to read commitments from blob".to_string())
        })
        .collect::<Result<_, _>>()?;

    let nullified = commitments
        .iter()
        .take(2)
        .filter(|commitment| commitment.0 != H256::zero())
        .copied()
        .collect();

    let created = commitments
        .iter()
        .skip(2)
        .filter(|commitment| commitment.0 != H256::zero())
        .copied()
        .collect();

    Ok((nullified, created))
}
