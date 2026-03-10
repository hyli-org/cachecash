use alloc::{format, vec::Vec};
use borsh::io::{self, Error, ErrorKind, Read, Write};
use core::{fmt, hash, iter, marker::PhantomData, ops::Deref};

use acvm::{AcirField, FieldElement};
use borsh::{BorshDeserialize, BorshSerialize};
use sparse_merkle_tree::{default_store::DefaultStore, traits::Value, SparseMerkleTree, H256};

#[cfg(test)]
use alloc::string::String;

#[derive(Debug)]
pub struct Poseidon2Hasher {
    buffer: Vec<FieldElement>,
}

impl Default for Poseidon2Hasher {
    fn default() -> Self {
        Self {
            buffer: Vec::with_capacity(8),
        }
    }
}

impl sparse_merkle_tree::traits::Hasher for Poseidon2Hasher {
    fn write_byte(&mut self, b: u8) {
        self.buffer.push(FieldElement::from(b as u128));
    }

    fn write_h256(&mut self, h: &H256) {
        let le = h.as_slice();
        let mut be = [0u8; 32];
        for i in 0..32 {
            be[31 - i] = le[i];
        }
        self.buffer.push(FieldElement::from_be_bytes_reduce(&be));
    }

    fn finish(self) -> H256 {
        let result = bn254_blackbox_solver::poseidon_hash(&self.buffer).unwrap();
        let le_bytes: [u8; 32] = result.to_le_bytes().try_into().unwrap();
        H256::from(le_bytes)
    }
}

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

impl hash::Hash for BorshableH256 {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        let hash_value = u64::from_le_bytes(self.0.as_slice()[..8].try_into().unwrap());
        state.write_u64(hash_value);
    }
}

impl fmt::Debug for BorshableH256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BorshableH256({})", hex::encode(self.0.as_slice()))
    }
}

impl BorshSerialize for BorshableH256 {
    fn serialize<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        let bytes: [u8; 32] = self.0.into();
        writer.write_all(&bytes)
    }
}

impl BorshDeserialize for BorshableH256 {
    fn deserialize_reader<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut bytes = [0u8; 32];
        reader.read_exact(&mut bytes)?;
        Ok(BorshableH256(H256::from(bytes)))
    }
}

impl Deref for BorshableH256 {
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
    SparseMerkleTree<Poseidon2Hasher, H256, DefaultStore<H256>>,
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

/// Convert an H256 (LE bytes) to a native FieldElement (LE→BE→field).
pub fn h256_to_field(h: &H256) -> FieldElement {
    let le = h.as_slice();
    let mut be = [0u8; 32];
    for i in 0..32 {
        be[31 - i] = le[i];
    }
    FieldElement::from_be_bytes_reduce(&be)
}

/// Build the flat 256-entry siblings array that the Noir circuit expects.
/// Each entry is the FieldElement representation of the sibling hash at that height,
/// or FieldElement::zero() if that level has no sibling.
pub fn build_siblings(tree: &SMT<BorshableH256>, commitment: BorshableH256) -> [FieldElement; 256] {
    let proof = tree.merkle_proof(iter::once(&commitment)).unwrap();
    let leaves_bitmap = proof.leaves_bitmap();
    let merkle_path = proof.merkle_path();

    let mut siblings = [FieldElement::zero(); 256];
    let mut path_idx = 0usize;
    for h in 0u32..256 {
        if leaves_bitmap[0].get_bit(h as u8) {
            let hash: H256 = merkle_path[path_idx].hash::<Poseidon2Hasher>();
            siblings[h as usize] = h256_to_field(&hash);
            path_idx += 1;
        }
    }
    siblings
}

impl BorshDeserialize for SMT<BorshableH256> {
    fn deserialize_reader<R: Read>(reader: &mut R) -> io::Result<Self> {
        let expected_root = BorshableH256::deserialize_reader(reader)?;
        let len: u32 = BorshDeserialize::deserialize_reader(reader)?;
        let mut tree = SMT::<BorshableH256>::zero();
        for _ in 0..len {
            let key = BorshableH256::deserialize_reader(reader)?;
            let value = BorshableH256::deserialize_reader(reader)?;
            tree.update_leaf(key, value)
                .map_err(|e| io::Error::other(format!("rebuilding SMT: {e}")))?;
        }
        if tree.root() != expected_root {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "SMT root mismatch during deserialization",
            ));
        }
        Ok(tree)
    }
}

#[cfg(test)]
pub mod smt_fixture {
    use super::*;

    /// Re-export for tests.
    pub use super::build_siblings;

    /// Print a Prover.toml for hyli_smt_incl_proof to stdout.
    /// Uses the new circuit format: blob has [nullifier0, nullifier1, notes_root],
    /// and input_notes provides note fields + secret key for commitment/nullifier computation.
    pub fn print_prover_toml() {
        // Known note fields (Field elements as BE hex)
        let kind = FieldElement::from(1u128); // non-zero = real note
        let value = FieldElement::from(100u128);
        let psi = FieldElement::from(42u128);
        let secret_key = FieldElement::from(7u128);
        let address =
            bn254_blackbox_solver::poseidon_hash(&[secret_key, FieldElement::zero()]).unwrap();

        // Commitment = poseidon2([0x2, kind, value, address, psi, 0, 0])
        let commitment = bn254_blackbox_solver::poseidon_hash(&[
            FieldElement::from(2u128),
            kind,
            value,
            address,
            psi,
            FieldElement::zero(),
            FieldElement::zero(),
        ])
        .unwrap();

        // Nullifier = poseidon2([psi, secret_key])
        let nullifier = bn254_blackbox_solver::poseidon_hash(&[psi, secret_key]).unwrap();

        // Padding nullifier = poseidon2([0, 0])
        let padding_nullifier =
            bn254_blackbox_solver::poseidon_hash(&[FieldElement::zero(), FieldElement::zero()])
                .unwrap();

        // Store commitment as BE bytes in SMT (matching how app.rs stores them)
        let commitment_be: [u8; 32] = commitment.to_be_bytes().try_into().unwrap();
        let commitment_h256 = BorshableH256::from(commitment_be);

        let mut tree = SMT::<BorshableH256>::zero();
        tree.update_leaf(commitment_h256, commitment_h256).unwrap();
        let root: [u8; 32] = tree.root().into();

        let siblings_0 = build_siblings(&tree, commitment_h256);
        let siblings_1 = [FieldElement::zero(); 256];

        // Build blob: [nullifier_0 (32B)][nullifier_1 (32B)][notes_root (32B)]
        let nullifier_be: [u8; 32] = nullifier.to_be_bytes().try_into().unwrap();
        let padding_null_be: [u8; 32] = padding_nullifier.to_be_bytes().try_into().unwrap();

        let mut blob = [0u8; 96];
        blob[..32].copy_from_slice(&nullifier_be);
        blob[32..64].copy_from_slice(&padding_null_be);
        blob[64..].copy_from_slice(&root);

        // --- helpers ---
        fn fmt_field_siblings(s: &[FieldElement; 256]) -> String {
            let rows: Vec<String> = s
                .iter()
                .map(|f| format!("\"0x{}\"", hex::encode(f.to_be_bytes())))
                .collect();
            format!("[{}]", rows.join(", "))
        }
        fn null_padded(s: &str, len: usize) -> String {
            let nulls: String = "\\u0000".repeat(len - s.len());
            format!("{}{}", s, nulls)
        }
        fn field_hex(f: &FieldElement) -> String {
            format!("\"0x{}\"", hex::encode(f.to_be_bytes()))
        }

        let contract_name = "hyli_smt_incl_proof";
        let identity = "test@hyli_smt";

        println!("# Auto-generated Prover.toml for hyli_smt_incl_proof (new format)");
        println!("# commitment_0 = {}", hex::encode(commitment_be));
        println!("# notes_root   = {}", hex::encode(root));
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
        println!(
            r#"blob_contract_name = "{}""#,
            null_padded(contract_name, 256)
        );
        println!();
        println!("blob_capacity = 96");
        println!("blob_len = 96");
        let blob_strs: Vec<String> = blob.iter().map(|x| x.to_string()).collect();
        println!("blob = [{}]", blob_strs.join(", "));
        println!();
        println!("tx_blob_count = 1");
        println!("success = true");
        println!();
        // Siblings must come before [[input_notes]] array-of-tables in TOML
        println!("siblings_0 = {}", fmt_field_siblings(&siblings_0));
        println!("siblings_1 = {}", fmt_field_siblings(&siblings_1));
        println!();
        // Input note 0: real note
        println!("[[input_notes]]");
        println!("secret_key = {}", field_hex(&secret_key));
        println!("[input_notes.note]");
        println!("kind = {}", field_hex(&kind));
        println!("value = {}", field_hex(&value));
        println!("address = {}", field_hex(&address));
        println!("psi = {}", field_hex(&psi));
        println!();
        // Input note 1: padding note
        println!("[[input_notes]]");
        println!(
            "secret_key = \"0x0000000000000000000000000000000000000000000000000000000000000000\""
        );
        println!("[input_notes.note]");
        println!("kind = \"0x0000000000000000000000000000000000000000000000000000000000000000\"");
        println!("value = \"0x0000000000000000000000000000000000000000000000000000000000000000\"");
        println!(
            "address = \"0x0000000000000000000000000000000000000000000000000000000000000000\""
        );
        println!("psi = \"0x0000000000000000000000000000000000000000000000000000000000000000\"");
    }

    #[test]
    fn generate_smt_prover_toml() {
        print_prover_toml();
    }
}
