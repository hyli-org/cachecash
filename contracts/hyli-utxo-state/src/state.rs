use borsh::{BorshDeserialize, BorshSerialize};
use sdk::{
    Calldata, RunResult, StateCommitment, merkle_utils::BorshableMerkleProof,
    utils::parse_raw_calldata,
};
use sparse_merkle_tree::H256;

use crate::zk::{
    Proof, ZkVmWitnessVec,
    smt::{BorshableH256, SMT, WitnessLeaf},
};

#[derive(Debug, Default, BorshSerialize, BorshDeserialize)]
pub struct HyliUtxoState {
    notes_tree: SMT<BorshableH256>,
    nullified_tree: SMT<BorshableH256>,
}

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, Default)]
pub struct HyliUtxoZkVmState {
    pub notes: ZkVmWitnessVec<WitnessLeaf>,
    pub nullified_notes: ZkVmWitnessVec<WitnessLeaf>,
}

#[derive(BorshSerialize)]
struct CommitmentSnapshot {
    notes_root: BorshableH256,
    nullified_notes_root: BorshableH256,
}

pub type HyliUtxoStateAction = [BorshableH256; 4];

impl HyliUtxoState {
    pub fn record_created(&mut self, commitments: &[BorshableH256]) -> Result<(), String> {
        for commitment in commitments {
            if commitment.0 == H256::zero() {
                continue;
            }

            if self.notes_tree.contains(commitment) {
                return Err("created note already exists in notes tree".to_string());
            }

            self.notes_tree
                .update_leaf(*commitment, *commitment)
                .map_err(|e| format!("failed to insert note into SMT: {e}"))?;
        }
        Ok(())
    }

    pub fn record_nullified(&mut self, commitments: &[BorshableH256]) -> Result<(), String> {
        for commitment in commitments {
            if commitment.0 == H256::zero() {
                continue;
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

    pub fn to_zkvm_state(
        &self,
        note_keys: &[BorshableH256],
        nullified_keys: &[BorshableH256],
    ) -> Result<HyliUtxoZkVmState, String> {
        let notes = Self::build_witness(&self.notes_tree, note_keys)?;
        let nullified = Self::build_witness(&self.nullified_tree, nullified_keys)?;

        Ok(HyliUtxoZkVmState {
            notes,
            nullified_notes: nullified,
        })
    }

    fn build_witness(
        tree: &SMT<BorshableH256>,
        keys: &[BorshableH256],
    ) -> Result<ZkVmWitnessVec<WitnessLeaf>, String> {
        if keys.is_empty() {
            return Ok(ZkVmWitnessVec::with_root(tree.root()));
        }

        let proof_inputs: Vec<WitnessLeaf> = keys
            .iter()
            .copied()
            .map(|key| WitnessLeaf::new(key, BorshableH256::from(H256::zero())))
            .collect();

        let proof = tree
            .merkle_proof(proof_inputs.iter())
            .map_err(|e| format!("failed to construct merkle proof: {e}"))?;

        let mut witness = ZkVmWitnessVec {
            values: Vec::with_capacity(keys.len()),
            proof: Proof::Some(BorshableMerkleProof::from(proof)),
        };

        for key in keys {
            let value = tree
                .store()
                .leaves_map()
                .get(&key.as_h256())
                .copied()
                .map(BorshableH256::from)
                .unwrap_or_else(|| BorshableH256::from(H256::zero()));
            witness.values.push(WitnessLeaf::new(*key, value));
        }

        Ok(witness)
    }

    pub fn commitment(&self) -> StateCommitment {
        let snapshot = CommitmentSnapshot {
            notes_root: self.notes_tree.root(),
            nullified_notes_root: self.nullified_tree.root(),
        };

        StateCommitment(
            borsh::to_vec(&snapshot).expect("state commitment serialization must succeed"),
        )
    }
}

impl sdk::FullStateRevert for HyliUtxoZkVmState {}

impl HyliUtxoZkVmState {
    fn apply_action(&mut self, action: &HyliUtxoStateAction) -> Result<(), String> {
        let created: Vec<_> = action
            .iter()
            .take(2)
            .copied()
            .filter(|c| c.0 != H256::zero())
            .collect();
        let nullified: Vec<_> = action
            .iter()
            .skip(2)
            .copied()
            .filter(|c| c.0 != H256::zero())
            .collect();

        if self.notes.values.len() != created.len() {
            return Err("notes witness entries do not match action size".to_string());
        }
        if self.nullified_notes.values.len() != nullified.len() {
            return Err("nullified witness entries do not match action size".to_string());
        }

        for (leaf, commitment) in self.notes.values.iter_mut().zip(created.iter()) {
            leaf.value = *commitment;
        }

        for (leaf, commitment) in self.nullified_notes.values.iter_mut().zip(nullified.iter()) {
            leaf.value = *commitment;
        }

        Ok(())
    }
}

impl sdk::ZkContract for HyliUtxoZkVmState {
    fn execute(&mut self, calldata: &Calldata) -> RunResult {
        let (action, ctx) = parse_raw_calldata::<HyliUtxoStateAction>(calldata)?;

        self.notes.ensure_all_zero()?;
        self.nullified_notes.ensure_all_zero()?;

        self.apply_action(&action)?;

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
