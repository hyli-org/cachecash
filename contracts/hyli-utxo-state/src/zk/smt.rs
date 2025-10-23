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
