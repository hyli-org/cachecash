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
        let bytes = h.as_slice();
        // Reverse LE → BE, then reduce into a single field element.
        let mut be: [u8; 32] = [0u8; 32];
        for i in 0..32 {
            be[31 - i] = bytes[i];
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

    pub fn leaf_count(&self) -> usize {
        self.0.store().leaves_map().len()
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
pub fn build_siblings(tree: &SMT<BorshableH256>, commitment: BorshableH256) -> [[u8; 32]; 256] {
    let proof = tree.merkle_proof(std::iter::once(&commitment)).unwrap();
    let leaves_bitmap = proof.leaves_bitmap();
    let merkle_path = proof.merkle_path();

    let mut siblings = [[0u8; 32]; 256];
    let mut path_idx = 0usize;
    for h in 0u32..256 {
        if leaves_bitmap[0].get_bit(h as u8) {
            let hash: [u8; 32] = merkle_path[path_idx].hash::<Poseidon2Hasher>().into();
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
        fn fmt_field_hex(le_bytes: &[u8; 32]) -> String {
            // Convert LE bytes to BE and format as hex field string
            let mut be = [0u8; 32];
            for i in 0..32 {
                be[31 - i] = le_bytes[i];
            }
            format!("\"0x{}\"", hex::encode(be))
        }
        fn fmt_siblings(s: &[[u8; 32]; 256]) -> String {
            let rows: Vec<String> = s.iter().map(|r| fmt_field_hex(r)).collect();
            format!("[{}]", rows.join(", "))
        }
        fn null_padded(s: &str, len: usize) -> String {
            let nulls: String = "\\u0000".repeat(len - s.len());
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
        println!("siblings_0 = {}", fmt_siblings(&siblings_0));
        println!("siblings_1 = {}", fmt_siblings(&siblings_1));
    }

    #[test]
    fn generate_smt_prover_toml() {
        // commitment = [1u8; 32] — a simple non-zero value
        print_prover_toml([1u8; 32]);
    }

    #[test]
    fn test_two_entry_tree() {
        // Two distinct commitments
        let c0 = [1u8; 32];
        let mut c1 = [2u8; 32];
        c1[31] = 3; // make sure they differ at a high bit

        let commitment0 = BorshableH256::from(c0);
        let commitment1 = BorshableH256::from(c1);

        let mut tree = SMT::<BorshableH256>::zero();
        tree.update_leaf(commitment0, commitment0).unwrap();
        tree.update_leaf(commitment1, commitment1).unwrap();

        let root: [u8; 32] = tree.root().into();
        let siblings_0 = build_siblings(&tree, commitment0);

        // Print non-zero siblings
        let non_zero: Vec<(usize, String)> = siblings_0.iter().enumerate()
            .filter(|(_, s)| s.iter().any(|b| *b != 0))
            .map(|(i, s)| (i, hex::encode(s)))
            .collect();
        eprintln!("root: {}", hex::encode(root));
        eprintln!("non-zero siblings: {:?}", non_zero);

        // Build blob and write Prover.toml
        let mut blob = [0u8; 96];
        blob[..32].copy_from_slice(&c0);
        blob[64..].copy_from_slice(&root);

        fn fmt_field_hex(le_bytes: &[u8; 32]) -> String {
            let mut be = [0u8; 32];
            for i in 0..32 { be[31 - i] = le_bytes[i]; }
            format!("\"0x{}\"", hex::encode(be))
        }
        fn fmt_siblings(s: &[[u8; 32]; 256]) -> String {
            let rows: Vec<String> = s.iter().map(|r| fmt_field_hex(r)).collect();
            format!("[{}]", rows.join(", "))
        }
        fn null_padded(s: &str, len: usize) -> String {
            format!("{}{}", s, "\\u0000".repeat(len - s.len()))
        }

        let contract_name = "hyli_smt_incl_proof";
        let identity = "test@hyli_smt";
        let siblings_1 = [[0u8; 32]; 256];

        println!("version = 1");
        println!("initial_state_len = 4");
        println!("initial_state = [0, 0, 0, 0]");
        println!("next_state_len = 4");
        println!("next_state = [0, 0, 0, 0]");
        println!("identity_len = {}", identity.len());
        println!(r#"identity = "{}""#, null_padded(identity, 256));
        println!(r#"tx_hash = "{}""#, "0".repeat(64));
        println!("index = 0");
        println!("blob_number = 1");
        println!("blob_index = 0");
        println!("blob_contract_name_len = {}", contract_name.len());
        println!(r#"blob_contract_name = "{}""#, null_padded(contract_name, 256));
        println!("blob_capacity = 96");
        println!("blob_len = 96");
        let blob_strs: Vec<String> = blob.iter().map(|x| x.to_string()).collect();
        println!("blob = [{}]", blob_strs.join(", "));
        println!("tx_blob_count = 1");
        println!("success = true");
        println!("siblings_0 = {}", fmt_siblings(&siblings_0));
        println!("siblings_1 = {}", fmt_siblings(&siblings_1));
    }

    #[test]
    fn reproduce_fe_failure() {
        // Exact values from FE console log
        let commitment_hex = "0800925b9d35134726fb7e1475eecb8351fc3e699e7ec6f8a31405e5481decbf";
        let fe_root_hex = "78bcc29b101334587951d9ad6f9d56aa197cbe8cf536840ca3c32981e3f9a903";

        let commitment_bytes: [u8; 32] = hex::decode(commitment_hex).unwrap().try_into().unwrap();
        let fe_root_bytes: [u8; 32] = hex::decode(fe_root_hex).unwrap().try_into().unwrap();

        let commitment = BorshableH256::from(commitment_bytes);
        let mut tree = SMT::<BorshableH256>::zero();
        tree.update_leaf(commitment, commitment).unwrap();

        let rust_root: [u8; 32] = tree.root().into();
        eprintln!("commitment bytes: {}", hex::encode(commitment_bytes));
        eprintln!("Rust root:        {}", hex::encode(rust_root));
        eprintln!("FE root:          {}", hex::encode(fe_root_bytes));
        eprintln!("Roots match:      {}", rust_root == fe_root_bytes);

        // Check siblings
        let siblings = build_siblings(&tree, commitment);
        let non_zero: Vec<(usize, String)> = siblings.iter().enumerate()
            .filter(|(_, s)| s.iter().any(|b| *b != 0))
            .map(|(i, s)| (i, hex::encode(s)))
            .collect();
        eprintln!("Non-zero siblings: {:?}", non_zero);

        // FE says siblings_0[254] LE bytes = [146, 149, 92, ...]
        let fe_sib_254: [u8; 32] = [146, 149, 92, 148, 158, 175, 60, 58, 110, 109, 104, 110, 127, 75, 34, 117, 160, 224, 80, 56, 145, 140, 220, 195, 2, 56, 46, 201, 94, 160, 4, 35];
        eprintln!("FE sib[254]:      {}", hex::encode(fe_sib_254));
        eprintln!("Rust sib[254]:    {}", hex::encode(siblings[254]));
        eprintln!("Sib[254] match:   {}", siblings[254] == fe_sib_254);

        // Simulate the circuit's verify_inclusion step by step
        eprintln!("\n--- Circuit simulation ---");

        // bytes32_to_field: interpret as LE integer
        fn bytes32_to_field(bytes: &[u8; 32]) -> FieldElement {
            let mut be = [0u8; 32];
            for i in 0..32 { be[31 - i] = bytes[i]; }
            FieldElement::from_be_bytes_reduce(&be)
        }

        fn get_bit(key: &[u8; 32], h: u8) -> u8 {
            let byte_pos = (h / 8) as usize;
            let bit_pos = h % 8;
            (key[byte_pos] >> bit_pos) & 1
        }

        let commitment_field = bytes32_to_field(&commitment_bytes);
        let notes_root_field = bytes32_to_field(&fe_root_bytes);
        eprintln!("commitment_field: {:?}", commitment_field);
        eprintln!("notes_root_field: {:?}", notes_root_field);

        // Convert fe_sib_254 to field (same as siblingsToFields)
        let sib_254_field = bytes32_to_field(&fe_sib_254);
        eprintln!("sib_254_field: {:?}", sib_254_field);

        // Run verify_inclusion
        let mut accumulated = FieldElement::zero();
        let mut power_of_2 = FieldElement::one();
        let two = FieldElement::from(2u128);

        let mut current_is_mwz = false;
        let mut current_value = commitment_field;
        let mut mwz_base = FieldElement::zero();
        let mut mwz_zero_bits = FieldElement::zero();
        let mut mwz_zero_count: u8 = 0;

        for h in 0u16..256 {
            let h_u8 = h as u8;
            let bit = get_bit(&commitment_bytes, h_u8);

            accumulated = accumulated + FieldElement::from(bit as u128) * power_of_2;
            let node_key = commitment_field - accumulated;

            let sib = if h == 254 { sib_254_field } else { FieldElement::zero() };

            let current_hash = if current_is_mwz {
                bn254_blackbox_solver::poseidon_hash(&[
                    FieldElement::from(2u128), mwz_base, mwz_zero_bits,
                    FieldElement::from(mwz_zero_count as u128)
                ]).unwrap()
            } else {
                current_value
            };

            if sib == FieldElement::zero() {
                let set_bit = bit == 1;
                if current_is_mwz {
                    if set_bit { mwz_zero_bits = mwz_zero_bits + power_of_2; }
                    mwz_zero_count = mwz_zero_count.wrapping_add(1);
                } else {
                    mwz_base = bn254_blackbox_solver::poseidon_hash(&[
                        FieldElement::from(h_u8 as u128), node_key, current_value
                    ]).unwrap();
                    mwz_zero_bits = FieldElement::zero();
                    if set_bit { mwz_zero_bits = power_of_2; }
                    mwz_zero_count = 1;
                    current_is_mwz = true;
                }
            } else {
                let lhs = if bit == 0 { current_hash } else { sib };
                let rhs = if bit == 0 { sib } else { current_hash };
                current_value = bn254_blackbox_solver::poseidon_hash(&[
                    FieldElement::one(), FieldElement::from(h_u8 as u128),
                    node_key, lhs, rhs
                ]).unwrap();
                current_is_mwz = false;
                eprintln!("h={}: merge_normal, bit={}, node_key={:?}", h, bit, node_key);
                eprintln!("  lhs={:?}", lhs);
                eprintln!("  rhs={:?}", rhs);
                eprintln!("  result={:?}", current_value);
            }

            power_of_2 = power_of_2 * two;
        }

        let computed_root = if current_is_mwz {
            bn254_blackbox_solver::poseidon_hash(&[
                FieldElement::from(2u128), mwz_base, mwz_zero_bits,
                FieldElement::from(mwz_zero_count as u128)
            ]).unwrap()
        } else {
            current_value
        };

        eprintln!("circuit computed_root: {:?}", computed_root);
        eprintln!("expected notes_root:  {:?}", notes_root_field);
        eprintln!("roots match: {}", computed_root == notes_root_field);

        // Also compute what the SMT library would produce for this tree
        // Build a 2-entry tree (the FE has 2 non-padding mints? or just 1?)
        // Try with just this commitment
        let mut tree1 = SMT::<BorshableH256>::zero();
        tree1.update_leaf(commitment, commitment).unwrap();
        let root1: [u8; 32] = tree1.root().into();
        let root1_field = bytes32_to_field(&root1);
        eprintln!("single-entry tree root field: {:?}", root1_field);

        // Generate Prover.toml using EXACT FE blob + siblings
        // (not from a fresh tree, but from the actual server data)
        let mut blob = [0u8; 96];
        blob[..32].copy_from_slice(&commitment_bytes);
        // blob[32..64] stays zero (padding commitment)
        blob[64..].copy_from_slice(&fe_root_bytes);

        let mut siblings_0 = [[0u8; 32]; 256];
        siblings_0[254] = fe_sib_254;
        let siblings_1 = [[0u8; 32]; 256]; // padding

        fn fmt_field_hex(le_bytes: &[u8; 32]) -> String {
            let mut be = [0u8; 32];
            for i in 0..32 { be[31 - i] = le_bytes[i]; }
            format!("\"0x{}\"", hex::encode(be))
        }
        fn fmt_siblings(s: &[[u8; 32]; 256]) -> String {
            let rows: Vec<String> = s.iter().map(|r| fmt_field_hex(r)).collect();
            format!("[{}]", rows.join(", "))
        }
        fn null_padded(s: &str, len: usize) -> String {
            format!("{}{}", s, "\\u0000".repeat(len - s.len()))
        }

        let contract_name = "hyli_smt_incl_proof";
        let identity = "test@hyli_smt";

        println!("version = 1");
        println!("initial_state_len = 4");
        println!("initial_state = [0, 0, 0, 0]");
        println!("next_state_len = 4");
        println!("next_state = [0, 0, 0, 0]");
        println!("identity_len = {}", identity.len());
        println!(r#"identity = "{}""#, null_padded(identity, 256));
        println!(r#"tx_hash = "{}""#, "0".repeat(64));
        println!("index = 0");
        println!("blob_number = 1");
        println!("blob_index = 0");
        println!("blob_contract_name_len = {}", contract_name.len());
        println!(r#"blob_contract_name = "{}""#, null_padded(contract_name, 256));
        println!("blob_capacity = 96");
        println!("blob_len = 96");
        let blob_strs: Vec<String> = blob.iter().map(|x| x.to_string()).collect();
        println!("blob = [{}]", blob_strs.join(", "));
        println!("tx_blob_count = 1");
        println!("success = true");
        println!("siblings_0 = {}", fmt_siblings(&siblings_0));
        println!("siblings_1 = {}", fmt_siblings(&siblings_1));
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
