use alloc::{
    collections::VecDeque,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::mem;

use acvm::FieldElement;
use borsh::{BorshDeserialize, BorshSerialize};
use hex;
use sdk::{
    caller::ExecutionContext, merkle_utils::BorshableMerkleProof, utils::parse_calldata, Calldata,
    ContractName, RunResult, StateCommitment, StructuredBlobData,
};
use sparse_merkle_tree::H256;

use crate::zk::{
    smt::{self as smt, BorshableH256, WitnessLeaf, SMT},
    Proof, ZkVmWitnessVec,
};

const MAX_ROOTS: usize = 1000;

#[derive(Debug, BorshSerialize, BorshDeserialize, Clone)]
pub struct ContractConfig {
    pub utxo_contract_name: ContractName,
    pub smt_incl_proof_contract_name: ContractName,
    pub smt_contract_name: ContractName,
}

#[derive(Debug, Default, BorshSerialize, BorshDeserialize)]
pub struct HyliUtxoState {
    notes_tree: SMT<BorshableH256>,
    nullified_tree: SMT<BorshableH256>,
    roots: VecDeque<[u8; 8]>,
}

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub struct HyliUtxoZkVmState {
    pub created_notes: ZkVmWitnessVec<WitnessLeaf>,
    pub nullified_notes: ZkVmWitnessVec<WitnessLeaf>,
    pub config: ContractConfig,
    pub roots: [[u8; 8]; MAX_ROOTS],
}

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
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
        merged.extend(next.remaining);
        merged.push(next.current);
        merged.extend(mem::take(&mut self.remaining));
        self.remaining = merged;
    }

    fn advance_step(&mut self) {
        if let Some(next) = self.remaining.pop() {
            self.current = next;
        } else {
            self.current = HyliUtxoZkVmState::new(self.current.config.clone());
        }
    }
}

#[derive(BorshSerialize)]
struct CommitmentSnapshot {
    notes_root: BorshableH256,
    nullified_notes_root: BorshableH256,
}

/// The action for the Hyli UTXO state is empty since all necessary information is passed through the Noir blobs in the calldata.
pub type HyliUtxoStateAction = [u8; 1];
pub const HYLI_UTXO_STATE_ACTION: HyliUtxoStateAction = [0];

/// The Hyli UTXO blob contains the commitments for the created notes and nullifiers for the nullified notes, in that order.
pub type HyliUtxoBlob = [BorshableH256; 4];

pub type SeparatedHyliUtxoBlob = ([BorshableH256; 2], [BorshableH256; 2]);

impl HyliUtxoState {
    pub fn update_roots(&mut self) {
        let new_root = self.notes_tree.root();
        let new_root: [u8; 8] = new_root.as_slice()[..8]
            .try_into()
            .expect("slice with incorrect length");
        self.roots.push_front(new_root);

        if self.roots.len() > MAX_ROOTS {
            self.roots.pop_back();
        }
    }

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
    pub const PADDING_NULLIFIER: [u8; 32] = [
        0x0b, 0x63, 0xa5, 0x37, 0x87, 0x02, 0x1a, 0x4a, 0x96, 0x2a, 0x45, 0x2c, 0x29, 0x21, 0xb3,
        0x66, 0x3a, 0xff, 0x1f, 0xfd, 0x8d, 0x55, 0x10, 0x54, 0x0f, 0x8e, 0x65, 0x9e, 0x78, 0x29,
        0x56, 0xf1,
    ];

    /// Filter out zero keys and padding nullifiers from a list of keys.
    /// Used by both `to_zkvm_state` and `apply_action` to ensure consistency.
    pub fn filter_keys(keys: &[BorshableH256], is_nullifier: bool) -> Vec<BorshableH256> {
        keys.iter()
            .filter(|k| {
                if k.0 == H256::zero() {
                    return false;
                }
                if is_nullifier {
                    let bytes: [u8; 32] = k.0.into();
                    return bytes != Self::PADDING_NULLIFIER;
                }
                true
            })
            .copied()
            .collect()
    }

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
        config: ContractConfig,
        created_note_keys: &[BorshableH256],
        nullified_keys: &[BorshableH256],
    ) -> Result<HyliUtxoZkVmState, String> {
        let filtered_created = Self::filter_keys(created_note_keys, false);
        let filtered_nullified = Self::filter_keys(nullified_keys, true);
        let created_notes = Self::build_witness(&self.notes_tree, &filtered_created)?;
        let nullified = Self::build_witness(&self.nullified_tree, &filtered_nullified)?;
        let mut roots_vec: Vec<[u8; 8]> = self.roots.iter().cloned().collect();
        roots_vec.resize(MAX_ROOTS, [0u8; 8]);
        let roots: [[u8; 8]; MAX_ROOTS] = roots_vec.try_into().map_err(|_| {
            format!(
                "failed to convert roots to array: expected length {}",
                MAX_ROOTS,
            )
        })?;

        Ok(HyliUtxoZkVmState {
            created_notes,
            nullified_notes: nullified,
            config,
            roots,
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

    pub fn notes_root(&self) -> BorshableH256 {
        self.notes_tree.root()
    }

    pub fn build_smt_witnesses(
        &self,
        commitment0: BorshableH256,
        commitment1: BorshableH256,
    ) -> ([FieldElement; 256], [FieldElement; 256]) {
        (
            smt::build_siblings(&self.notes_tree, commitment0),
            smt::build_siblings(&self.notes_tree, commitment1),
        )
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
    pub fn new(config: ContractConfig) -> Self {
        Self {
            config,
            created_notes: Default::default(),
            nullified_notes: Default::default(),
            roots: [[0u8; 8]; MAX_ROOTS],
        }
    }

    fn parse_smt_incl_blob_payload(blob: &sdk::Blob) -> Result<Vec<u8>, String> {
        let structured: StructuredBlobData<Vec<u8>> =
            blob.data.clone().try_into().map_err(|_| {
                "failed to parse hyli_smt_incl_proof blob as structured blob".to_string()
            })?;
        Ok(structured.parameters)
    }

    fn check_noir_blobs(
        &self,
        calldata: &Calldata,
        ctx: &mut ExecutionContext,
    ) -> Result<(), String> {
        let Some((_, hyli_utxo_blob)) = calldata
            .blobs
            .iter()
            .find(|(_, blob)| blob.contract_name == self.config.utxo_contract_name)
        else {
            return Err("hyli_utxo_noir blob not provided in calldata".to_string());
        };

        let Some(smt_incl_blob_index) = ctx
            .callees_blobs
            .iter()
            .position(|blob| blob.contract_name == self.config.smt_incl_proof_contract_name)
        else {
            return Err(
                "hyli_smt_incl_proof_noir callee blob not provided in calldata".to_string(),
            );
        };
        let smt_incl_blob = ctx.callees_blobs.remove(smt_incl_blob_index);

        // Step 1: Check that the smt_incl_blob's notes root matches the computed notes root from the witness.
        let smt_blob_payload = Self::parse_smt_incl_blob_payload(&smt_incl_blob)?;
        let (smt_nullifier0, smt_nullifier1, smt_blob_notes_root) =
            parse_hyli_smt_incl_blob(&smt_blob_payload)?;

        if !self.roots.contains(smt_blob_notes_root) {
            return Err("smt inclusion proof blob does not match notes root".to_string());
        }

        // Step 2: Check that the nullifiers in the smt_incl_blob match those in the utxo blob.
        let (_, utxo_nullifiers) = parse_hyli_utxo_blob(&hyli_utxo_blob.data.0)?;

        if utxo_nullifiers[0] != smt_nullifier0 {
            return Err(
                "hyli_utxo_blob nullifier 0 does not match smt inclusion proof nullifier 0"
                    .to_string(),
            );
        }
        if utxo_nullifiers[1] != smt_nullifier1 {
            return Err(
                "hyli_utxo_blob nullifier 1 does not match smt inclusion proof nullifier 1"
                    .to_string(),
            );
        }

        // Optional step 3: Check that the blob callee topology matches the expected topology for a withdraw transaction:
        let withdraw_callees = ctx
            .callees_blobs
            .iter()
            .enumerate()
            .filter(|(_, blob)| blob.contract_name == self.config.smt_contract_name)
            .collect::<Vec<_>>();

        if withdraw_callees.len() > 1 {
            return Err("multiple withdraw callees found for hyli-utxo-state blob".to_string());
        }
        if let Some((token_blob_index, _)) = withdraw_callees.first() {
            ctx.callees_blobs.remove(*token_blob_index);
        }
        if !ctx.callees_blobs.is_empty() {
            return Err(format!(
                "hyli-utxo-state callee set mismatch: unexpected remaining callees {:?}",
                ctx.callees_blobs
            ));
        }

        Ok(())
    }

    fn apply_action(&mut self, calldata: &Calldata) -> Result<(), String> {
        let (_, hyli_utxo_blob) = calldata
            .blobs
            .iter()
            .find(|(_, blob)| blob.contract_name == self.config.utxo_contract_name)
            .ok_or_else(|| "hyli_utxo blob not found in calldata".to_string())?;

        let (created, nullified) = parse_hyli_utxo_blob(&hyli_utxo_blob.data.0)
            .map_err(|e| format!("failed to parse hyli_utxo blob: {e}"))?;

        let filtered_created = HyliUtxoState::filter_keys(&created, false);
        let filtered_nullified = HyliUtxoState::filter_keys(&nullified, true);

        if self.created_notes.values.len() != filtered_created.len() {
            return Err(format!(
                "notes witness entries ({}) do not match filtered action size ({})",
                self.created_notes.values.len(),
                filtered_created.len()
            ));
        }
        if self.nullified_notes.values.len() != filtered_nullified.len() {
            return Err(format!(
                "nullified witness entries ({}) do not match filtered action size ({})",
                self.nullified_notes.values.len(),
                filtered_nullified.len()
            ));
        }

        for (leaf, commitment) in self
            .created_notes
            .values
            .iter_mut()
            .zip(filtered_created.iter())
        {
            leaf.value = *commitment;
        }

        for (leaf, commitment) in self
            .nullified_notes
            .values
            .iter_mut()
            .zip(filtered_nullified.iter())
        {
            leaf.value = *commitment;
        }

        Ok(())
    }
}

impl sdk::ZkContract for HyliUtxoZkVmState {
    fn execute(&mut self, calldata: &Calldata) -> RunResult {
        let (_, mut ctx) = parse_calldata::<HyliUtxoStateAction>(calldata)?;

        self.created_notes.ensure_all_zero()?;
        self.nullified_notes.ensure_all_zero()?;

        self.check_noir_blobs(calldata, &mut ctx)?;

        self.apply_action(calldata)?;

        Ok((Vec::new(), ctx, Vec::new()))
    }

    fn commit(&self) -> StateCommitment {
        let notes_root = self
            .created_notes
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

pub fn parse_hyli_utxo_blob(bytes: &[u8]) -> Result<SeparatedHyliUtxoBlob, String> {
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

    let output_notes = [commitments[0], commitments[1]];
    let nullifiers = [commitments[2], commitments[3]];

    Ok((output_notes, nullifiers))
}

pub fn parse_hyli_smt_incl_blob(
    bytes: &[u8],
) -> Result<(BorshableH256, BorshableH256, &[u8; 8]), String> {
    const EXPECTED_SIZE: usize = 96;
    if bytes.len() != EXPECTED_SIZE {
        return Err(format!(
            "hyli_smt_incl blob must be {EXPECTED_SIZE} bytes, found {}",
            bytes.len()
        ));
    }

    let nullifier0 = BorshableH256::from(
        <[u8; 32]>::try_from(&bytes[0..32])
            .map_err(|_| "Failed to read nullifier0 from smt blob".to_string())?,
    );
    let nullifier1 = BorshableH256::from(
        <[u8; 32]>::try_from(&bytes[32..64])
            .map_err(|_| "Failed to read nullifier1 from smt blob".to_string())?,
    );

    // Only extract fingerprint for notes root (first 8 bytes) since that's the only one we need to verify in the zk contract.
    let notes_root_fingerprint = bytes[64..(64 + 8)]
        .try_into()
        .map_err(|_| "Failed to read notes root fingerprint from smt blob".to_string())?;

    Ok((nullifier0, nullifier1, notes_root_fingerprint))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdk::{Blob, BlobData, BlobIndex, ContractName, StructuredBlobData, TxHash};

    fn state_with_root(byte: u8) -> HyliUtxoZkVmState {
        let mut state = HyliUtxoZkVmState::new(ContractConfig {
            utxo_contract_name: "dummy_utxo".into(),
            smt_incl_proof_contract_name: "dummy_smt_incl".into(),
            smt_contract_name: "oranj".into(),
        });
        let root = BorshableH256::from([byte; 32]);
        state.created_notes.proof = Proof::CurrentRootHash(root);
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
            match &batch.current.created_notes.proof {
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

    fn make_state_blob(callees: Vec<BlobIndex>) -> Blob {
        Blob {
            contract_name: ContractName("hyli-utxo-state".into()),
            data: BlobData::from(StructuredBlobData {
                caller: None,
                callees: Some(callees),
                parameters: HYLI_UTXO_STATE_ACTION,
            }),
        }
    }

    fn make_utxo_blob(nullifier_byte: u8) -> Blob {
        let mut bytes = vec![0u8; 128];
        bytes[64..96].copy_from_slice(&[nullifier_byte; 32]);
        bytes[96..128].copy_from_slice(&[nullifier_byte.wrapping_add(1); 32]);
        Blob {
            contract_name: ContractName("dummy_utxo".into()),
            data: BlobData(bytes),
        }
    }

    fn make_smt_blob(root_byte: u8, nullifier_byte: u8) -> Blob {
        let mut bytes = vec![0u8; 96];
        bytes[0..32].copy_from_slice(&[nullifier_byte; 32]);
        bytes[32..64].copy_from_slice(&[nullifier_byte.wrapping_add(1); 32]);
        bytes[64..96].copy_from_slice(&[root_byte; 32]);
        Blob {
            contract_name: ContractName("dummy_smt_incl".into()),
            data: BlobData::from(StructuredBlobData {
                caller: Some(BlobIndex(0)),
                callees: None,
                parameters: bytes,
            }),
        }
    }

    fn make_token_blob(caller: Option<BlobIndex>) -> Blob {
        Blob {
            contract_name: ContractName("oranj".into()),
            data: BlobData::from(StructuredBlobData {
                caller,
                callees: None,
                parameters: vec![1u8, 2, 3],
            }),
        }
    }

    #[test]
    fn check_noir_blobs_accepts_withdraw_topology() {
        let mut state = state_with_root(7);
        state.roots[0] = [7u8; 8];

        let calldata = sdk::Calldata {
            tx_hash: TxHash(vec![0u8; 32]),
            identity: "alice".into(),
            blobs: vec![
                make_state_blob(vec![BlobIndex(2), BlobIndex(3)]),
                make_utxo_blob(9),
                make_smt_blob(7, 9),
                make_token_blob(Some(BlobIndex(0))),
            ]
            .into(),
            tx_blob_count: 4,
            index: BlobIndex(0),
            tx_ctx: None,
            private_input: Vec::new(),
        };
        let (_, mut ctx) =
            parse_calldata::<HyliUtxoStateAction>(&calldata).expect("parse state calldata");

        state
            .check_noir_blobs(&calldata, &mut ctx)
            .expect("withdraw topology should be accepted");
    }

    #[test]
    fn check_noir_blobs_rejects_missing_withdraw_token_callee() {
        let mut state = state_with_root(7);
        state.roots[0] = [7u8; 8];

        let calldata = sdk::Calldata {
            tx_hash: TxHash(vec![0u8; 32]),
            identity: "alice".into(),
            blobs: vec![
                make_state_blob(vec![BlobIndex(2)]),
                make_utxo_blob(9),
                make_smt_blob(7, 9),
                make_token_blob(Some(BlobIndex(0))),
            ]
            .into(),
            tx_blob_count: 4,
            index: BlobIndex(0),
            tx_ctx: None,
            private_input: Vec::new(),
        };
        let err = parse_calldata::<HyliUtxoStateAction>(&calldata)
            .expect_err("withdraw topology should fail during calldata parsing");
        assert!(err.contains("Blob callees do not match actual callees"));
    }
}
