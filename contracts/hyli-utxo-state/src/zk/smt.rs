use std::{io, marker::PhantomData};

use borsh::{BorshDeserialize, BorshSerialize};
use sdk::merkle_utils::SHA256Hasher;
use sparse_merkle_tree::{default_store::DefaultStore, traits::Value, SparseMerkleTree, H256};

pub trait GetKey {
    fn get_key(&self) -> BorshableH256;
}

impl<T: GetKey> GetKey for &T {
    fn get_key(&self) -> BorshableH256 {
        (*self).get_key()
    }
}

#[derive(Default, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct BorshableH256(pub H256);

impl BorshableH256 {
    pub fn as_h256(&self) -> H256 {
        self.0
    }
}

impl GetKey for BorshableH256 {
    fn get_key(&self) -> BorshableH256 {
        *self
    }
}

impl Value for BorshableH256 {
    fn to_h256(&self) -> H256 {
        self.0
    }

    fn zero() -> Self {
        BorshableH256(H256::zero())
    }
}

#[derive(
    Debug, Clone, Copy, Default, Eq, PartialEq, PartialOrd, Ord, BorshSerialize, BorshDeserialize,
)]
pub struct WitnessLeaf {
    pub key: BorshableH256,
    pub value: BorshableH256,
}

impl WitnessLeaf {
    pub fn new(key: BorshableH256, value: BorshableH256) -> Self {
        Self { key, value }
    }
}

impl GetKey for WitnessLeaf {
    fn get_key(&self) -> BorshableH256 {
        self.key
    }
}

impl Value for WitnessLeaf {
    fn to_h256(&self) -> H256 {
        self.value.0
    }

    fn zero() -> Self {
        Self::default()
    }
}

impl std::hash::Hash for BorshableH256 {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let hash_value = u64::from_le_bytes(self.0.as_slice()[..8].try_into().unwrap());
        state.write_u64(hash_value);
    }
}

impl std::fmt::Debug for BorshableH256 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BorshableH256({})", hex::encode(self.0.as_slice()))
    }
}

impl BorshSerialize for BorshableH256 {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let bytes: [u8; 32] = self.0.into();
        writer.write_all(&bytes)
    }
}

impl BorshDeserialize for BorshableH256 {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let mut bytes = [0u8; 32];
        reader.read_exact(&mut bytes)?;
        Ok(BorshableH256(H256::from(bytes)))
    }
}

impl std::ops::Deref for BorshableH256 {
    type Target = H256;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<[u8]> for BorshableH256 {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl From<[u8; 32]> for BorshableH256 {
    fn from(bytes: [u8; 32]) -> Self {
        BorshableH256(bytes.into())
    }
}

impl From<H256> for BorshableH256 {
    fn from(h: H256) -> Self {
        BorshableH256(h)
    }
}

impl From<BorshableH256> for H256 {
    fn from(h: BorshableH256) -> Self {
        h.0
    }
}

impl From<BorshableH256> for [u8; 32] {
    fn from(h: BorshableH256) -> Self {
        h.0.into()
    }
}

impl<'a> From<&'a H256> for &'a BorshableH256 {
    fn from(h: &'a H256) -> &'a BorshableH256 {
        unsafe { &*(h as *const H256 as *const BorshableH256) }
    }
}

#[derive(Debug, Default)]
pub struct SMT<T: Value + Clone>(
    SparseMerkleTree<SHA256Hasher, H256, DefaultStore<H256>>,
    PhantomData<T>,
);

impl<T> SMT<T>
where
    T: Value + Clone,
{
    pub fn zero() -> Self {
        SMT(
            SparseMerkleTree::new(H256::zero(), Default::default()),
            PhantomData,
        )
    }

    pub fn from_store(root: BorshableH256, store: DefaultStore<H256>) -> Self {
        SMT(SparseMerkleTree::new(root.into(), store), PhantomData)
    }

    pub fn update_all_from_ref<'a, I>(
        &mut self,
        leaves: I,
    ) -> sparse_merkle_tree::error::Result<BorshableH256>
    where
        I: Iterator<Item = &'a T>,
        T: Value + GetKey + 'a,
    {
        let h256_leaves = leaves.map(|el| (el.get_key().0, el.to_h256())).collect();
        self.0.update_all(h256_leaves).map(|r| BorshableH256(*r))
    }

    pub fn update_all<I>(&mut self, leaves: I) -> sparse_merkle_tree::error::Result<BorshableH256>
    where
        I: Iterator<Item = T>,
        T: Value + GetKey,
    {
        let h256_leaves = leaves
            .map(|el| (el.get_key().0, el.to_h256()))
            .collect::<Vec<_>>();
        self.0.update_all(h256_leaves).map(|r| BorshableH256(*r))
    }

    pub fn update_leaf(
        &mut self,
        key: BorshableH256,
        value: T,
    ) -> sparse_merkle_tree::error::Result<BorshableH256> {
        self.0
            .update(*key, value.to_h256())
            .map(|r| BorshableH256(*r))
    }

    pub fn contains(&self, key: &BorshableH256) -> bool {
        let store = self.0.store();
        let h = key.as_h256();
        store.leaves_map().contains_key(&h)
    }

    pub fn root(&self) -> BorshableH256 {
        BorshableH256(*self.0.root())
    }

    pub fn store(&self) -> &DefaultStore<H256> {
        self.0.store()
    }

    pub fn merkle_proof<'a, I, V>(
        &self,
        keys: I,
    ) -> sparse_merkle_tree::error::Result<sparse_merkle_tree::merkle_proof::MerkleProof>
    where
        I: Iterator<Item = &'a V>,
        V: Value + GetKey + 'a,
    {
        self.0
            .merkle_proof(keys.map(|v| v.get_key().0).collect::<Vec<_>>())
    }
}

impl BorshSerialize for SMT<BorshableH256> {
    fn serialize<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        self.root().serialize(writer)?;
        let leaves = self.store().leaves_map();
        let len = leaves.len() as u32;
        len.serialize(writer)?;
        for (key, value) in leaves.iter() {
            BorshableH256(*key).serialize(writer)?;
            BorshableH256(*value).serialize(writer)?;
        }
        Ok(())
    }
}

/// Build the flat 256-entry siblings array that the Noir circuit expects.
/// Entry h is the hash of the sibling MergeValue at height h, or [0u8;32]
/// if that level has no sibling (all-zero path).
pub fn build_siblings(
    tree: &SMT<BorshableH256>,
    commitment: BorshableH256,
) -> [[u8; 32]; 256] {
    use sdk::merkle_utils::SHA256Hasher;
    let proof = tree.merkle_proof(std::iter::once(&commitment)).unwrap();
    let leaves_bitmap = proof.leaves_bitmap();
    let merkle_path = proof.merkle_path();

    let mut siblings = [[0u8; 32]; 256];
    let mut path_idx = 0usize;
    for h in 0u32..256 {
        if leaves_bitmap[0].get_bit(h as u8) {
            let hash: [u8; 32] = merkle_path[path_idx].hash::<SHA256Hasher>().into();
            siblings[h as usize] = hash;
            path_idx += 1;
        }
    }
    siblings
}

#[cfg(test)]
pub mod smt_fixture {
    use super::*;

    /// Re-export for tests.
    pub use super::build_siblings;

    /// Print a Prover.toml for hyli_smt_incl_proof to stdout.
    /// commitment_0 is the real note; commitment_1 is the zero padding note.
    pub fn print_prover_toml(commitment_bytes: [u8; 32]) {
        let commitment = BorshableH256::from(commitment_bytes);

        let mut tree = SMT::<BorshableH256>::zero();
        tree.update_leaf(commitment, commitment).unwrap();
        let root: [u8; 32] = tree.root().into();

        let siblings_0 = build_siblings(&tree, commitment);
        let siblings_1 = [[0u8; 32]; 256]; // padding note — all-zero siblings

        // Build blob: [commitment_0 (32)] [commitment_1=0 (32)] [notes_root (32)]
        let mut blob = [0u8; 96];
        blob[..32].copy_from_slice(&commitment_bytes);
        // blob[32..64] stays zero (padding commitment)
        blob[64..].copy_from_slice(&root);

        // --- helpers ---
        fn fmt_bytes32(b: &[u8; 32]) -> String {
            let s: Vec<String> = b.iter().map(|x| x.to_string()).collect();
            format!("[{}]", s.join(", "))
        }
        fn fmt_siblings(s: &[[u8; 32]; 256]) -> String {
            let rows: Vec<String> = s.iter().map(|r| fmt_bytes32(r)).collect();
            format!("[{}]", rows.join(", "))
        }
        fn null_padded(s: &str, len: usize) -> String {
            let nulls: String = std::iter::repeat('\0').take(len - s.len()).collect();
            format!("{}{}", s, nulls)
        }

        let contract_name = "hyli_smt_incl_proof";
        let identity = "test@hyli_smt";

        println!("# Auto-generated Prover.toml for hyli_smt_incl_proof");
        println!("# commitment_0 = {}", hex::encode(&commitment_bytes));
        println!("# commitment_1 = 0000..00 (padding)");
        println!("# notes_root   = {}", hex::encode(&root));
        println!();
        println!("version = 1");
        println!("initial_state_len = 4");
        println!("initial_state = [0, 0, 0, 0]");
        println!("next_state_len = 4");
        println!("next_state = [0, 0, 0, 0]");
        println!();
        println!("identity_len = {}", identity.len());
        println!(r#"identity = "{}""#, null_padded(identity, 256));
        println!();
        println!(r#"tx_hash = "{}""#, "0".repeat(64));
        println!();
        println!("index = 0");
        println!("blob_number = 1");
        println!("blob_index = 0");
        println!();
        println!("blob_contract_name_len = {}", contract_name.len());
        println!(r#"blob_contract_name = "{}""#, null_padded(contract_name, 256));
        println!();
        println!("blob_capacity = 96");
        println!("blob_len = 96");
        let blob_strs: Vec<String> = blob.iter().map(|x| x.to_string()).collect();
        println!("blob = [{}]", blob_strs.join(", "));
        println!();
        println!("tx_blob_count = 1");
        println!("success = true");
        println!();
        println!("siblings_0 = {}", fmt_siblings(&siblings_0));
        println!("siblings_1 = {}", fmt_siblings(&siblings_1));
    }

    #[test]
    fn generate_smt_prover_toml() {
        // commitment = [1u8; 32] — a simple non-zero value
        print_prover_toml([1u8; 32]);
    }
}

impl BorshDeserialize for SMT<BorshableH256> {
    fn deserialize_reader<R: io::Read>(reader: &mut R) -> io::Result<Self> {
        let expected_root = BorshableH256::deserialize_reader(reader)?;
        let len: u32 = BorshDeserialize::deserialize_reader(reader)?;
        let mut tree = SMT::<BorshableH256>::zero();
        for _ in 0..len {
            let key = BorshableH256::deserialize_reader(reader)?;
            let value = BorshableH256::deserialize_reader(reader)?;
            tree.update_leaf(key, value).map_err(|e| {
                io::Error::new(io::ErrorKind::Other, format!("rebuilding SMT: {e}"))
            })?;
        }
        if tree.root() != expected_root {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "SMT root mismatch during deserialization",
            ));
        }
        Ok(tree)
    }
}
