use borsh::{BorshDeserialize, BorshSerialize};
use sdk::merkle_utils::{BorshableMerkleProof, SHA256Hasher};
use sparse_merkle_tree::{traits::Value, H256};

pub mod smt;

pub use smt::{BorshableH256, GetKey, WitnessLeaf, SMT};

#[derive(Debug, Clone, BorshDeserialize, BorshSerialize)]
pub enum Proof {
    Some(BorshableMerkleProof),
    CurrentRootHash(BorshableH256),
}

impl Default for Proof {
    fn default() -> Self {
        Proof::CurrentRootHash(BorshableH256::default())
    }
}

#[derive(Debug, Clone, BorshDeserialize, BorshSerialize)]
pub struct ZkVmWitnessVec<
    T: BorshDeserialize + BorshSerialize + Default + Value + GetKey + Ord + Clone,
> {
    pub values: Vec<T>,
    pub proof: Proof,
}

impl<T: BorshDeserialize + BorshSerialize + Default + Value + GetKey + Ord + Clone> Default
    for ZkVmWitnessVec<T>
{
    fn default() -> Self {
        Self {
            values: Vec::new(),
            proof: Proof::default(),
        }
    }
}

impl<T: BorshDeserialize + BorshSerialize + Default + Value + GetKey + Ord + Clone>
    ZkVmWitnessVec<T>
{
    pub fn with_root(root: BorshableH256) -> Self {
        Self {
            values: Vec::new(),
            proof: Proof::CurrentRootHash(root),
        }
    }

    pub fn contains(&self, value: &T) -> bool {
        let key = value.get_key();
        self.values
            .iter()
            .any(|candidate| candidate.get_key() == key)
    }

    pub fn insert(&mut self, value: T) -> bool {
        let key = value.get_key();
        if let Some(existing) = self
            .values
            .iter_mut()
            .find(|candidate| candidate.get_key() == key)
        {
            *existing = value;
            false
        } else {
            self.values.push(value);
            true
        }
    }

    pub fn compute_root(&self) -> Result<BorshableH256, String> {
        match &self.proof {
            Proof::CurrentRootHash(root) => Ok(*root),
            Proof::Some(proof) => {
                let leaves: Vec<_> = self
                    .values
                    .iter()
                    .map(|v| (v.get_key().0, v.to_h256()))
                    .collect();

                if leaves.is_empty() {
                    return Err(
                        "Witness set values are empty while a proof is provided".to_string()
                    );
                }

                proof
                    .0
                    .clone()
                    .compute_root::<SHA256Hasher>(leaves)
                    .map(|root| BorshableH256::from(root))
                    .map_err(|e| format!("Failed to compute SMT root from proof: {e}"))
            }
        }
    }

    pub fn ensure_all_zero(&self) -> Result<(), String> {
        if self
            .values
            .iter()
            .any(|value| value.to_h256() != H256::zero())
        {
            return Err("witness values must all be zero".to_string());
        }
        Ok(())
    }
}
