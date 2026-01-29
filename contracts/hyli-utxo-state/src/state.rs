use borsh::{BorshDeserialize, BorshSerialize};
use hex;
use sdk::{
    merkle_utils::BorshableMerkleProof, utils::parse_raw_calldata, Calldata, RunResult,
    StateCommitment,
};
use sparse_merkle_tree::H256;

use crate::zk::{
    smt::{BorshableH256, WitnessLeaf, SMT},
    Proof, ZkVmWitnessVec,
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

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, Default)]
pub struct HyliUtxoZkVmBatch {
    pub current: HyliUtxoZkVmState,
    pub remaining: Vec<HyliUtxoZkVmState>,
}

impl HyliUtxoZkVmBatch {
    pub fn from_state(state: HyliUtxoZkVmState) -> Self {
        Self {
            current: state,
            remaining: Vec::new(),
        }
    }

    pub fn extend_with(&mut self, next: HyliUtxoZkVmBatch) {
        let mut merged = Vec::with_capacity(next.remaining.len() + 1 + self.remaining.len());
        merged.extend(next.remaining.into_iter());
        merged.push(next.current);
        merged.extend(std::mem::take(&mut self.remaining));
        self.remaining = merged;
    }

    fn advance_step(&mut self) {
        if let Some(next) = self.remaining.pop() {
            self.current = next;
        } else {
            self.current = HyliUtxoZkVmState::default();
        }
    }
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

    /// The padding nullifier is poseidon2([0, 0], 2) - this is a well-known constant
    /// that results from using a padding note (psi=0, secret_key=0).
    /// We must skip this value to allow multiple transactions with padding notes.
    const PADDING_NULLIFIER: [u8; 32] = [
        0x0b, 0x63, 0xa5, 0x37, 0x87, 0x02, 0x1a, 0x4a,
        0x96, 0x2a, 0x45, 0x2c, 0x29, 0x21, 0xb3, 0x66,
        0x3a, 0xff, 0x1f, 0xfd, 0x8d, 0x55, 0x10, 0x54,
        0x0f, 0x8e, 0x65, 0x9e, 0x78, 0x29, 0x56, 0xf1,
    ];

    pub fn record_nullified(&mut self, commitments: &[BorshableH256]) -> Result<(), String> {
        for (i, commitment) in commitments.iter().enumerate() {
            if commitment.0 == H256::zero() {
                continue;
            }

            // Skip the padding nullifier - it's poseidon2([0, 0], 2) and is used
            // by all transactions that have only 1 real input note.
            let commitment_bytes: [u8; 32] = commitment.0.into();
            if commitment_bytes == Self::PADDING_NULLIFIER {
                continue;
            }

            // Debug: print the nullifier being checked
            let nullifier_hex = hex::encode(commitment.0.as_slice());

            if self.nullified_tree.contains(commitment) {
                return Err(format!(
                    "note has already been nullified: nullifier[{}] = {}",
                    i, nullifier_hex
                ));
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
    /// The padding nullifier constant - must match HyliUtxoState::PADDING_NULLIFIER
    const PADDING_NULLIFIER: [u8; 32] = [
        0x0b, 0x63, 0xa5, 0x37, 0x87, 0x02, 0x1a, 0x4a,
        0x96, 0x2a, 0x45, 0x2c, 0x29, 0x21, 0xb3, 0x66,
        0x3a, 0xff, 0x1f, 0xfd, 0x8d, 0x55, 0x10, 0x54,
        0x0f, 0x8e, 0x65, 0x9e, 0x78, 0x29, 0x56, 0xf1,
    ];

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
            .filter(|c| {
                let bytes: [u8; 32] = c.0.into();
                c.0 != H256::zero() && bytes != Self::PADDING_NULLIFIER
            })
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

impl sdk::ZkContract for HyliUtxoZkVmBatch {
    fn execute(&mut self, calldata: &Calldata) -> RunResult {
        <HyliUtxoZkVmState as sdk::ZkContract>::execute(&mut self.current, calldata)
    }

    fn commit(&self) -> StateCommitment {
        <HyliUtxoZkVmState as sdk::ZkContract>::commit(&self.current)
    }
}

impl sdk::TransactionalZkContract for HyliUtxoZkVmBatch {
    type State = Self;

    fn initial_state(&self) -> Self::State {
        self.clone()
    }

    fn revert(&mut self, initial_state: Self::State) {
        *self = initial_state;
    }

    fn on_success(&mut self) -> StateCommitment {
        let commitment = <HyliUtxoZkVmState as sdk::ZkContract>::commit(&self.current);
        self.advance_step();
        commitment
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

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_root(byte: u8) -> HyliUtxoZkVmState {
        let mut state = HyliUtxoZkVmState::default();
        let root = BorshableH256::from([byte; 32]);
        state.notes.proof = Proof::CurrentRootHash(root);
        state.nullified_notes.proof = Proof::CurrentRootHash(BorshableH256::from([byte; 32]));
        state
    }

    #[test]
    fn batch_advances_steps_in_fifo_order() {
        let s1 = state_with_root(1);
        let s2 = state_with_root(2);
        let s3 = state_with_root(3);

        let mut batch = HyliUtxoZkVmBatch::from_state(s1.clone());
        batch.extend_with(HyliUtxoZkVmBatch::from_state(s2.clone()));
        batch.extend_with(HyliUtxoZkVmBatch::from_state(s3.clone()));

        fn assert_root(batch: &HyliUtxoZkVmBatch, expected: u8) {
            match &batch.current.notes.proof {
                Proof::CurrentRootHash(root) => {
                    let actual: [u8; 32] = (*root).into();
                    assert_eq!(actual, [expected; 32]);
                }
                Proof::Some(_) => panic!("expected CurrentRootHash proof"),
            }
        }

        assert_root(&batch, 1);
        assert_eq!(batch.remaining.len(), 2);

        // Advance to next step; commitment should still reflect the applied step.
        let _ = <HyliUtxoZkVmBatch as sdk::TransactionalZkContract>::on_success(&mut batch);
        assert_root(&batch, 2);
        assert_eq!(batch.remaining.len(), 1);

        let _ = <HyliUtxoZkVmBatch as sdk::TransactionalZkContract>::on_success(&mut batch);
        assert_root(&batch, 3);
        assert_eq!(batch.remaining.len(), 0);
    }
}
