use std::{io, marker::PhantomData};

use acvm::{AcirField, FieldElement};
use borsh::{BorshDeserialize, BorshSerialize};
use sparse_merkle_tree::{default_store::DefaultStore, traits::Value, SparseMerkleTree, H256};

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
    let proof = tree.merkle_proof(std::iter::once(&commitment)).unwrap();
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
    fn deserialize_reader<R: io::Read>(reader: &mut R) -> io::Result<Self> {
        let expected_root = BorshableH256::deserialize_reader(reader)?;
        let len: u32 = BorshDeserialize::deserialize_reader(reader)?;
        let mut tree = SMT::<BorshableH256>::zero();
        for _ in 0..len {
            let key = BorshableH256::deserialize_reader(reader)?;
            let value = BorshableH256::deserialize_reader(reader)?;
            tree.update_leaf(key, value).map_err(|e| {
                io::Error::other(format!("rebuilding SMT: {e}"))
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
        let kind     = FieldElement::from(1u128);  // non-zero = real note
        let value    = FieldElement::from(100u128);
        let psi      = FieldElement::from(42u128);
        let secret_key = FieldElement::from(7u128);
        let address  = bn254_blackbox_solver::poseidon_hash(&[secret_key, FieldElement::zero()]).unwrap();

        // Commitment = poseidon2([0x2, kind, value, address, psi, 0, 0])
        let commitment = bn254_blackbox_solver::poseidon_hash(&[
            FieldElement::from(2u128), kind, value, address, psi,
            FieldElement::zero(), FieldElement::zero(),
        ]).unwrap();

        // Nullifier = poseidon2([psi, secret_key])
        let nullifier = bn254_blackbox_solver::poseidon_hash(&[psi, secret_key]).unwrap();

        // Padding nullifier = poseidon2([0, 0])
        let padding_nullifier = bn254_blackbox_solver::poseidon_hash(&[
            FieldElement::zero(), FieldElement::zero(),
        ]).unwrap();

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
        println!("secret_key = \"0x0000000000000000000000000000000000000000000000000000000000000000\"");
        println!("[input_notes.note]");
        println!("kind = \"0x0000000000000000000000000000000000000000000000000000000000000000\"");
        println!("value = \"0x0000000000000000000000000000000000000000000000000000000000000000\"");
        println!("address = \"0x0000000000000000000000000000000000000000000000000000000000000000\"");
        println!("psi = \"0x0000000000000000000000000000000000000000000000000000000000000000\"");
    }

    #[test]
    fn generate_smt_prover_toml() {
        print_prover_toml();
    }

    #[test]
    fn verify_fe_commitment() {
        // Exact note fields from FE debug log
        let kind_hex = "000200000000000000893c499c542cef5e3811e1192ce70d8cc03d5c33590000";
        let value_hex = "000000000000000000000000000000000000000000000000000000000000000a";
        let address_hex = "25f5b472e3c8eb4800b1ed5f4ae57f30e476a14b5c9d3d43a6beeab40c00a369";
        let psi_hex = "2a1858c816953d1565727da8a6f0d94083ee2a4ae6020cf286952da9d7066d47";

        fn hex_to_field(h: &str) -> FieldElement {
            let bytes = hex::decode(h).unwrap();
            FieldElement::from_be_bytes_reduce(&bytes)
        }

        let kind = hex_to_field(kind_hex);
        let value = hex_to_field(value_hex);
        let address = hex_to_field(address_hex);
        let psi = hex_to_field(psi_hex);

        // Same as Note::commitment() and Noir circuit get_note_commitment
        let commitment = bn254_blackbox_solver::poseidon_hash(&[
            FieldElement::from(2u128), kind, value, address, psi,
            FieldElement::zero(), FieldElement::zero(),
        ]).unwrap();

        let commit_hex = hex::encode(commitment.to_be_bytes());
        println!("Rust commitment: {}", commit_hex);
        println!("FE   commitment: 04a951f67e66d783c991546fc0469ea19e63239db96e64cb2688e14a27b081bb");

        // Also compute what the SMT stores
        let commit_be: [u8; 32] = commitment.to_be_bytes().try_into().unwrap();
        let commit_h256 = BorshableH256::from(commit_be);
        println!("SMT key (H256 hex): {}", hex::encode(commit_h256.0.as_slice()));
    }

    /// Simulate the Noir circuit's verify_inclusion algorithm in Rust,
    /// using the exact same hash functions (poseidon2) and byte conventions.
    #[test]
    fn simulate_noir_verify_inclusion_real_data() {
        // --- Real data from FE debug output ---
        let commitment_be_hex = "04a951f67e66d783c991546fc0469ea19e63239db96e64cb2688e14a27b081bb";
        let notes_root_le_hex = "ce03a806a4be2133eae29514e0eeee9cfdacca698129adcdafe61dae32bfd012";
        let sibling_255_be_hex = "29adb7e753681b9f1ea3497802fefccbf2732f03adc94972d31e4eec5506f08d";

        let commitment_be: [u8; 32] = hex::decode(commitment_be_hex).unwrap().try_into().unwrap();
        let notes_root_le: [u8; 32] = hex::decode(notes_root_le_hex).unwrap().try_into().unwrap();

        // notes_root as field element (LE interpretation)
        let notes_root_field = h256_to_field(&H256::from(notes_root_le));

        // sibling at height 255 as field element
        let sib_255 = FieldElement::from_be_bytes_reduce(
            &hex::decode(sibling_255_be_hex).unwrap(),
        );
        let mut siblings = [FieldElement::zero(); 256];
        siblings[255] = sib_255;

        // --- Noir helper functions implemented in Rust ---
        fn noir_h256_to_field(bytes: &[u8; 32]) -> FieldElement {
            let mut be = [0u8; 32];
            for i in 0..32 {
                be[31 - i] = bytes[i];
            }
            FieldElement::from_be_bytes_reduce(&be)
        }

        fn noir_get_bit(key: &[u8; 32], h: u8) -> u8 {
            let byte_pos = (h / 8) as usize;
            let bit_pos = h % 8;
            ((key[byte_pos] >> bit_pos) & 1)
        }

        fn noir_parent_path(key: &[u8; 32], h: u8) -> [u8; 32] {
            if h == 255 {
                return [0u8; 32];
            }
            let start = h + 1;
            let start_byte = start / 8;
            let remain = start % 8;
            let mut result = [0u8; 32];
            for i in 0..32 {
                if i as u8 > start_byte {
                    result[i] = key[i];
                }
            }
            if (start_byte as usize) < 32 {
                result[start_byte as usize] = key[start_byte as usize] & (0xffu8 << remain);
            }
            result
        }

        fn noir_hash_base_node(height: u8, node_key: &[u8; 32], value: FieldElement) -> FieldElement {
            let nk = noir_h256_to_field(node_key);
            bn254_blackbox_solver::poseidon_hash(&[
                FieldElement::from(height as u128), nk, value,
            ]).unwrap()
        }

        fn noir_hash_mwz(base_node: FieldElement, zero_bits: &[u8; 32], zero_count: u8) -> FieldElement {
            let zb = noir_h256_to_field(zero_bits);
            bn254_blackbox_solver::poseidon_hash(&[
                FieldElement::from(2u128), base_node, zb, FieldElement::from(zero_count as u128),
            ]).unwrap()
        }

        fn noir_hash_merge_normal(height: u8, node_key: &[u8; 32], lhs: FieldElement, rhs: FieldElement) -> FieldElement {
            let nk = noir_h256_to_field(node_key);
            bn254_blackbox_solver::poseidon_hash(&[
                FieldElement::from(1u128), FieldElement::from(height as u128), nk, lhs, rhs,
            ]).unwrap()
        }

        // --- Simulate verify_inclusion ---
        let commitment_smt_value = noir_h256_to_field(&commitment_be);

        let mut current_is_mwz = false;
        let mut current_value = commitment_smt_value;
        let mut mwz_base = FieldElement::zero();
        let mut mwz_zero_bits = [0u8; 32];
        let mut mwz_zero_count: u8 = 0;

        for h in 0u16..256 {
            let h_u8 = h as u8;
            let node_key = noir_parent_path(&commitment_be, h_u8);
            let bit = noir_get_bit(&commitment_be, h_u8);
            let sib = siblings[h as usize];

            let sib_is_zero = sib == FieldElement::zero();

            let current_hash = if current_is_mwz {
                noir_hash_mwz(mwz_base, &mwz_zero_bits, mwz_zero_count)
            } else {
                current_value
            };

            if sib_is_zero {
                let set_bit = bit == 1;
                if current_is_mwz {
                    if set_bit {
                        mwz_zero_bits[(h_u8 / 8) as usize] |= 1 << (h_u8 % 8);
                    }
                    mwz_zero_count = mwz_zero_count.wrapping_add(1);
                } else {
                    mwz_base = noir_hash_base_node(h_u8, &node_key, current_value);
                    mwz_zero_bits = [0u8; 32];
                    if set_bit {
                        mwz_zero_bits[(h_u8 / 8) as usize] |= 1 << (h_u8 % 8);
                    }
                    mwz_zero_count = 1;
                    current_is_mwz = true;
                }
            } else {
                println!("Height {}: normal merge, bit={}, current_hash={:?}", h, bit, hex::encode(current_hash.to_be_bytes()));
                println!("  sib = {:?}", hex::encode(sib.to_be_bytes()));
                let lhs = if bit == 0 { current_hash } else { sib };
                let rhs = if bit == 0 { sib } else { current_hash };
                println!("  lhs = {:?}", hex::encode(lhs.to_be_bytes()));
                println!("  rhs = {:?}", hex::encode(rhs.to_be_bytes()));
                current_value = noir_hash_merge_normal(h_u8, &node_key, lhs, rhs);
                current_is_mwz = false;
                println!("  result = {:?}", hex::encode(current_value.to_be_bytes()));
            }
        }

        let computed_root = if current_is_mwz {
            noir_hash_mwz(mwz_base, &mwz_zero_bits, mwz_zero_count)
        } else {
            current_value
        };

        println!("Computed root (BE hex): {}", hex::encode(computed_root.to_be_bytes()));
        println!("Expected root (field): {}", hex::encode(notes_root_field.to_be_bytes()));
        println!("mwz_zero_count at h=254: {}", mwz_zero_count);
        assert_eq!(computed_root, notes_root_field, "SMT inclusion proof failed in Rust simulation!");
    }

    /// Test the circuit simulation with a known 2-leaf tree
    #[test]
    fn simulate_noir_with_two_leaf_tree() {
        fn noir_h256_to_field(bytes: &[u8; 32]) -> FieldElement {
            let mut be = [0u8; 32];
            for i in 0..32 { be[31 - i] = bytes[i]; }
            FieldElement::from_be_bytes_reduce(&be)
        }
        fn noir_get_bit(key: &[u8; 32], h: u8) -> u8 {
            let byte_pos = (h / 8) as usize;
            let bit_pos = h % 8;
            (key[byte_pos] >> bit_pos) & 1
        }
        fn noir_parent_path(key: &[u8; 32], h: u8) -> [u8; 32] {
            if h == 255 { return [0u8; 32]; }
            let start = h + 1;
            let start_byte = start / 8;
            let remain = start % 8;
            let mut result = [0u8; 32];
            for i in 0..32 {
                if i as u8 > start_byte { result[i] = key[i]; }
            }
            if (start_byte as usize) < 32 {
                result[start_byte as usize] = key[start_byte as usize] & (0xffu8 << remain);
            }
            result
        }
        fn noir_hash_base_node(height: u8, node_key: &[u8; 32], value: FieldElement) -> FieldElement {
            let nk = noir_h256_to_field(node_key);
            bn254_blackbox_solver::poseidon_hash(&[
                FieldElement::from(height as u128), nk, value,
            ]).unwrap()
        }
        fn noir_hash_mwz(base_node: FieldElement, zero_bits: &[u8; 32], zero_count: u8) -> FieldElement {
            let zb = noir_h256_to_field(zero_bits);
            bn254_blackbox_solver::poseidon_hash(&[
                FieldElement::from(2u128), base_node, zb, FieldElement::from(zero_count as u128),
            ]).unwrap()
        }
        fn noir_hash_merge_normal(height: u8, node_key: &[u8; 32], lhs: FieldElement, rhs: FieldElement) -> FieldElement {
            let nk = noir_h256_to_field(node_key);
            bn254_blackbox_solver::poseidon_hash(&[
                FieldElement::from(1u128), FieldElement::from(height as u128), nk, lhs, rhs,
            ]).unwrap()
        }

        // Create 2 commitments
        let commit_a = bn254_blackbox_solver::poseidon_hash(&[
            FieldElement::from(2u128), FieldElement::from(1u128), FieldElement::from(100u128),
            FieldElement::from(42u128), FieldElement::from(7u128),
            FieldElement::zero(), FieldElement::zero(),
        ]).unwrap();
        let commit_b = bn254_blackbox_solver::poseidon_hash(&[
            FieldElement::from(2u128), FieldElement::from(1u128), FieldElement::from(200u128),
            FieldElement::from(99u128), FieldElement::from(13u128),
            FieldElement::zero(), FieldElement::zero(),
        ]).unwrap();

        let commit_a_be: [u8; 32] = commit_a.to_be_bytes().try_into().unwrap();
        let commit_b_be: [u8; 32] = commit_b.to_be_bytes().try_into().unwrap();
        let h256_a = BorshableH256::from(commit_a_be);
        let h256_b = BorshableH256::from(commit_b_be);

        let mut tree = SMT::<BorshableH256>::zero();
        tree.update_leaf(h256_a, h256_a).unwrap();
        tree.update_leaf(h256_b, h256_b).unwrap();

        let root: [u8; 32] = tree.root().into();
        let root_field = h256_to_field(&H256::from(root));

        let siblings = build_siblings(&tree, h256_a);
        let non_zero: Vec<(usize, String)> = siblings.iter().enumerate()
            .filter(|(_, s)| **s != FieldElement::zero())
            .map(|(i, s)| (i, format!("0x{}", hex::encode(s.to_be_bytes()))))
            .collect();
        println!("2-leaf tree root (LE hex): {}", hex::encode(root));
        println!("Non-zero siblings for commit_a: {:?}", non_zero);

        // Now simulate the Noir circuit
        let commitment_smt_value = noir_h256_to_field(&commit_a_be);
        let mut current_is_mwz = false;
        let mut current_value = commitment_smt_value;
        let mut mwz_base = FieldElement::zero();
        let mut mwz_zero_bits = [0u8; 32];
        let mut mwz_zero_count: u8 = 0;

        for h in 0u16..256 {
            let h_u8 = h as u8;
            let node_key = noir_parent_path(&commit_a_be, h_u8);
            let bit = noir_get_bit(&commit_a_be, h_u8);
            let sib = siblings[h as usize];
            let sib_is_zero = sib == FieldElement::zero();

            let current_hash = if current_is_mwz {
                noir_hash_mwz(mwz_base, &mwz_zero_bits, mwz_zero_count)
            } else {
                current_value
            };

            if sib_is_zero {
                let set_bit = bit == 1;
                if current_is_mwz {
                    if set_bit {
                        mwz_zero_bits[(h_u8 / 8) as usize] |= 1 << (h_u8 % 8);
                    }
                    mwz_zero_count = mwz_zero_count.wrapping_add(1);
                } else {
                    mwz_base = noir_hash_base_node(h_u8, &node_key, current_value);
                    mwz_zero_bits = [0u8; 32];
                    if set_bit {
                        mwz_zero_bits[(h_u8 / 8) as usize] |= 1 << (h_u8 % 8);
                    }
                    mwz_zero_count = 1;
                    current_is_mwz = true;
                }
            } else {
                println!("h={}: normal merge, bit={}", h, bit);
                let lhs = if bit == 0 { current_hash } else { sib };
                let rhs = if bit == 0 { sib } else { current_hash };
                current_value = noir_hash_merge_normal(h_u8, &node_key, lhs, rhs);
                current_is_mwz = false;
            }
        }

        let computed_root = if current_is_mwz {
            noir_hash_mwz(mwz_base, &mwz_zero_bits, mwz_zero_count)
        } else {
            current_value
        };

        println!("Computed root (BE): {}", hex::encode(computed_root.to_be_bytes()));
        println!("Expected root (BE): {}", hex::encode(root_field.to_be_bytes()));
        assert_eq!(computed_root, root_field, "2-leaf tree simulation failed!");
    }

    /// Verify build_siblings + circuit simulation is always consistent
    #[test]
    fn verify_build_siblings_consistency_various_sizes() {
        fn noir_h256_to_field(bytes: &[u8; 32]) -> FieldElement {
            let mut be = [0u8; 32];
            for i in 0..32 { be[31 - i] = bytes[i]; }
            FieldElement::from_be_bytes_reduce(&be)
        }
        fn noir_get_bit(key: &[u8; 32], h: u8) -> u8 {
            (key[(h / 8) as usize] >> (h % 8)) & 1
        }
        fn noir_parent_path(key: &[u8; 32], h: u8) -> [u8; 32] {
            if h == 255 { return [0u8; 32]; }
            let start = h + 1;
            let start_byte = start / 8;
            let remain = start % 8;
            let mut result = [0u8; 32];
            for i in 0..32 {
                if i as u8 > start_byte { result[i] = key[i]; }
            }
            if (start_byte as usize) < 32 {
                result[start_byte as usize] = key[start_byte as usize] & (0xffu8 << remain);
            }
            result
        }

        fn simulate_verify(commitment_be: &[u8; 32], notes_root_field: FieldElement, siblings: &[FieldElement; 256]) -> bool {
            let commitment_smt_value = noir_h256_to_field(commitment_be);
            let mut current_is_mwz = false;
            let mut current_value = commitment_smt_value;
            let mut mwz_base = FieldElement::zero();
            let mut mwz_zero_bits = [0u8; 32];
            let mut mwz_zero_count: u8 = 0;

            for h in 0u16..256 {
                let h_u8 = h as u8;
                let node_key = noir_parent_path(commitment_be, h_u8);
                let bit = noir_get_bit(commitment_be, h_u8);
                let sib = siblings[h as usize];
                let sib_is_zero = sib == FieldElement::zero();

                let current_hash = if current_is_mwz {
                    let zb = noir_h256_to_field(&mwz_zero_bits);
                    bn254_blackbox_solver::poseidon_hash(&[
                        FieldElement::from(2u128), mwz_base, zb, FieldElement::from(mwz_zero_count as u128),
                    ]).unwrap()
                } else {
                    current_value
                };

                if sib_is_zero {
                    let set_bit = bit == 1;
                    if current_is_mwz {
                        if set_bit { mwz_zero_bits[(h_u8 / 8) as usize] |= 1 << (h_u8 % 8); }
                        mwz_zero_count = mwz_zero_count.wrapping_add(1);
                    } else {
                        let nk = noir_h256_to_field(&node_key);
                        mwz_base = bn254_blackbox_solver::poseidon_hash(&[
                            FieldElement::from(h as u128), nk, current_value,
                        ]).unwrap();
                        mwz_zero_bits = [0u8; 32];
                        if set_bit { mwz_zero_bits[(h_u8 / 8) as usize] |= 1 << (h_u8 % 8); }
                        mwz_zero_count = 1;
                        current_is_mwz = true;
                    }
                } else {
                    let lhs = if bit == 0 { current_hash } else { sib };
                    let rhs = if bit == 0 { sib } else { current_hash };
                    let nk = noir_h256_to_field(&node_key);
                    current_value = bn254_blackbox_solver::poseidon_hash(&[
                        FieldElement::from(1u128), FieldElement::from(h as u128), nk, lhs, rhs,
                    ]).unwrap();
                    current_is_mwz = false;
                }
            }

            let computed_root = if current_is_mwz {
                let zb = noir_h256_to_field(&mwz_zero_bits);
                bn254_blackbox_solver::poseidon_hash(&[
                    FieldElement::from(2u128), mwz_base, zb, FieldElement::from(mwz_zero_count as u128),
                ]).unwrap()
            } else {
                current_value
            };

            computed_root == notes_root_field
        }

        // Create commitments
        let mut commits = Vec::new();
        for i in 0..10u128 {
            let c = bn254_blackbox_solver::poseidon_hash(&[
                FieldElement::from(2u128), FieldElement::from(i + 1),
                FieldElement::from(100 + i), FieldElement::from(42 + i),
                FieldElement::from(7 + i), FieldElement::zero(), FieldElement::zero(),
            ]).unwrap();
            commits.push(c);
        }

        // Test with trees of size 1, 2, 3, 5, 10
        for size in [1, 2, 3, 5, 10] {
            let mut tree = SMT::<BorshableH256>::zero();
            for c in &commits[..size] {
                let be: [u8; 32] = c.to_be_bytes().try_into().unwrap();
                let h = BorshableH256::from(be);
                tree.update_leaf(h, h).unwrap();
            }
            let root: [u8; 32] = tree.root().into();
            let root_field = h256_to_field(&H256::from(root));

            // Verify each commitment
            for (j, c) in commits[..size].iter().enumerate() {
                let be: [u8; 32] = c.to_be_bytes().try_into().unwrap();
                let h = BorshableH256::from(be);
                let siblings = build_siblings(&tree, h);
                let ok = simulate_verify(&be, root_field, &siblings);
                assert!(ok, "FAILED: tree size={}, leaf index={}", size, j);
            }
            println!("OK: tree size={}, all {} leaves verified", size, size);
        }
    }

    /// Also verify: build the same tree and compare with the Noir simulation
    #[test]
    fn verify_real_tree_root_and_siblings() {
        let commitment_be_hex = "04a951f67e66d783c991546fc0469ea19e63239db96e64cb2688e14a27b081bb";
        let notes_root_le_hex = "ce03a806a4be2133eae29514e0eeee9cfdacca698129adcdafe61dae32bfd012";

        let commitment_be: [u8; 32] = hex::decode(commitment_be_hex).unwrap().try_into().unwrap();
        let notes_root_le: [u8; 32] = hex::decode(notes_root_le_hex).unwrap().try_into().unwrap();
        let commitment_h256 = BorshableH256::from(commitment_be);

        // Build a single-leaf tree
        let mut tree = SMT::<BorshableH256>::zero();
        tree.update_leaf(commitment_h256, commitment_h256).unwrap();
        let single_root: [u8; 32] = tree.root().into();
        println!("Single-leaf tree root (LE hex): {}", hex::encode(single_root));
        println!("Expected notes_root (LE hex):   {}", hex::encode(notes_root_le));

        if single_root == notes_root_le {
            println!("Root matches single-leaf tree!");
        } else {
            println!("Root does NOT match - tree has more leaves");
        }

        let siblings = build_siblings(&tree, commitment_h256);
        let non_zero: Vec<(usize, String)> = siblings.iter().enumerate()
            .filter(|(_, s)| **s != FieldElement::zero())
            .map(|(i, s)| (i, format!("0x{}", hex::encode(s.to_be_bytes()))))
            .collect();
        println!("Single-leaf tree non-zero siblings: {:?}", non_zero);
    }
}
