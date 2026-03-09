use dep::poseidon::poseidon2::Poseidon2;
use common::{compute_nullifier, get_note_commitment, InputNote};
use std::ops::WrappingAdd;

// ---------------------------------------------------------------------------
// SMT hash helpers (Poseidon2-based, ZK-native)
// ---------------------------------------------------------------------------

// Convert a 32-byte LE H256 to a single Field element (LE interpretation).
// Only needed for byte-level constructs (node_key, zero_bits), not hash outputs.
fn h256_to_field(input: [u8; 32]) -> Field {
    let mut result: Field = 0;
    let mut scale: Field = 1;
    for i in 0..32 {
        result += (input[i] as Field) * scale;
        scale *= 256;
    }
    result
}

// Poseidon2([height, nk, v], 3) -- nk is byte-pattern (needs conversion), v is Field
fn hash_base_node(height: u8, node_key: [u8; 32], value: Field) -> Field {
    let nk = h256_to_field(node_key);
    Poseidon2::hash([height as Field, nk, value], 3)
}

// Poseidon2([2, bn, zb, zero_count], 4) -- bn is Field, zb is byte-pattern
fn hash_mwz(base_node: Field, zero_bits: [u8; 32], zero_count: u8) -> Field {
    let zb = h256_to_field(zero_bits);
    Poseidon2::hash([2, base_node, zb, zero_count as Field], 4)
}

// Poseidon2([1, height, nk, l, r], 5) -- nk is byte-pattern, l/r are Fields
fn hash_merge_normal(height: u8, node_key: [u8; 32], lhs: Field, rhs: Field) -> Field {
    let nk = h256_to_field(node_key);
    Poseidon2::hash([1, height as Field, nk, lhs, rhs], 5)
}

// Bit at position h (LSB-first within each byte, like H256::get_bit)
fn get_bit(key: [u8; 32], h: u8) -> u1 {
    let byte_pos: u8 = h / 8;
    let bit_pos: u8 = h % 8;
    ((key[byte_pos as u32] >> bit_pos) & 1) as u1
}

// Copy bits (h+1)..=255 of key, zero bits 0..=h.
// Special case: h == 255 -- return zero.
fn parent_path(key: [u8; 32], h: u8) -> [u8; 32] {
    if h == 255 {
        [0u8; 32]
    } else {
        let start: u8 = h + 1; // first bit to keep
        let start_byte: u8 = start / 8;
        let remain: u8 = start % 8;
        let mut result: [u8; 32] = [0; 32];
        // copy full bytes above start_byte
        for i in 0..32 {
            if i as u8 > start_byte {
                result[i] = key[i];
            }
        }
        // partial byte at start_byte: keep only bits >= remain
        if start_byte < 32 {
            result[start_byte as u32] = key[start_byte as u32] & (0xff_u8 << remain);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Single-leaf SMT inclusion proof
// ---------------------------------------------------------------------------

fn verify_inclusion(
    commitment_field: Field,
    notes_root: Field,
    siblings: [Field; 256],
) {
    // Handle padding note: if commitment is zero, skip verification.
    if commitment_field != 0 {
        // Get BE bytes matching the Rust SMT key storage: BorshableH256::from(field.to_be_bytes())
        let commitment_bytes: [u8; 32] = commitment_field.to_be_bytes();

        // The SMT hasher sees these bytes via write_h256 which interprets them as LE integer.
        let commitment_smt_value = h256_to_field(commitment_bytes);

        // Accumulator state -- all hash values stay as Field
        let mut current_is_mwz: bool = false;
        let mut current_value: Field = commitment_smt_value;
        let mut mwz_base: Field = 0;
        let mut mwz_zero_bits: [u8; 32] = [0; 32];
        let mut mwz_zero_count: u8 = 0;

        for h in 0..256 {
            let h_u8 = h as u8;
            let node_key = parent_path(commitment_bytes, h_u8);
            let bit = get_bit(commitment_bytes, h_u8);
            let sib = siblings[h];

            // Is sibling zero?
            let sib_is_zero = (sib == 0);

            // Hash of current accumulator
            let current_hash: Field = if current_is_mwz {
                hash_mwz(mwz_base, mwz_zero_bits, mwz_zero_count)
            } else {
                current_value
            };

            if sib_is_zero {
                // Merge with zero: current goes into MergeWithZero
                // set_bit = true when current is on the RIGHT (bit==1), meaning zero is on LEFT
                let set_bit = (bit == 1);
                if current_is_mwz {
                    // Extend existing MergeWithZero
                    if set_bit {
                        mwz_zero_bits[h_u8 as u32 / 8] |= 1 << (h_u8 % 8);
                    }
                    mwz_zero_count = mwz_zero_count.wrapping_add(1);
                } else {
                    // Create new MergeWithZero
                    mwz_base = hash_base_node(h_u8, node_key, current_value);
                    mwz_zero_bits = [0u8; 32];
                    if set_bit {
                        mwz_zero_bits[h_u8 as u32 / 8] |= 1 << (h_u8 % 8);
                    }
                    mwz_zero_count = 1;
                    current_is_mwz = true;
                }
            } else {
                // Normal merge: both sides non-zero -- lhs/rhs are already Fields
                let lhs: Field = if bit == 0 { current_hash } else { sib };
                let rhs: Field = if bit == 0 { sib } else { current_hash };
                current_value = hash_merge_normal(h_u8, node_key, lhs, rhs);
                current_is_mwz = false;
            }
        }

        // Final computed root
        let computed_root: Field = if current_is_mwz {
            hash_mwz(mwz_base, mwz_zero_bits, mwz_zero_count)
        } else {
            current_value
        };

        assert(computed_root == notes_root, "SMT inclusion proof failed");
    }
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

fn main(
    // Hyli-required public plumbing
    version: pub u32,
    initial_state_len: pub u32,
    initial_state: pub [u8; 4],
    next_state_len: pub u32,
    next_state: pub [u8; 4],
    identity_len: pub u8,
    identity: pub str<256>,
    tx_hash: pub str<64>,
    index: pub u32,
    blob_number: pub u32,
    blob_index: pub u32,
    blob_contract_name_len: pub u8,
    blob_contract_name: pub str<256>,
    blob_capacity: pub u32,
    blob_len: pub u32,
    blob: pub [u8; 96],
    tx_blob_count: pub u32,
    success: pub bool,
    // Private inputs: input notes (note data + secret key for nullifier computation)
    input_notes: [InputNote; 2],
    // Private SMT proof witnesses
    siblings_0: [Field; 256],
    siblings_1: [Field; 256],
) {
    // -------------------------------------------------------------------------
    // Hyli contract framing (same pattern as hyli_utxo)
    // -------------------------------------------------------------------------
    assert(success, "contract execution must succeed");
    assert(version == 1, "unsupported Hyli output version");

    assert(initial_state_len == 4, "initial_state must be 4 bytes");
    assert(next_state_len == 4, "next_state must be 4 bytes");

    assert(blob_capacity == 96, "blob capacity must be 96 bytes");
    assert(blob_len == 96, "blob length must be 96 bytes");

    assert(identity_len as u32 <= 256, "identity length overflow");
    assert(blob_contract_name_len == 19, "blob contract name must be 19 bytes");

    let _ = initial_state;
    let _ = next_state;
    let _ = identity;
    let _ = tx_hash;
    let _ = blob_number;
    let _ = blob_index;
    let _ = index;
    let _ = tx_blob_count;
    let _ = blob_contract_name;

    // -------------------------------------------------------------------------
    // Parse blob: [nullifier_0 (32B)][nullifier_1 (32B)][notes_root (32B)]
    // -------------------------------------------------------------------------
    let mut blob_nullifier_0: [u8; 32] = [0; 32];
    let mut blob_nullifier_1: [u8; 32] = [0; 32];
    let mut notes_root_bytes: [u8; 32] = [0; 32];

    for i in 0..32 {
        blob_nullifier_0[i] = blob[i];
        blob_nullifier_1[i] = blob[32 + i];
        notes_root_bytes[i] = blob[64 + i];
    }

    // Convert notes_root to Field (LE interpretation of stored bytes)
    let notes_root = h256_to_field(notes_root_bytes);

    // -------------------------------------------------------------------------
    // Compute commitments and nullifiers from input notes
    // -------------------------------------------------------------------------
    let commitment_0 = get_note_commitment(input_notes[0].note);
    let commitment_1 = get_note_commitment(input_notes[1].note);

    let nullifier_0 =
        compute_nullifier(input_notes[0].note.psi, input_notes[0].secret_key).to_be_bytes();
    let nullifier_1 =
        compute_nullifier(input_notes[1].note.psi, input_notes[1].secret_key).to_be_bytes();

    assert(nullifier_0 == blob_nullifier_0, "nullifier_0 must match blob");
    assert(nullifier_1 == blob_nullifier_1, "nullifier_1 must match blob");

    // -------------------------------------------------------------------------
    // Verify SMT inclusion for both input note commitments
    // -------------------------------------------------------------------------
    verify_inclusion(commitment_0, notes_root, siblings_0);
    verify_inclusion(commitment_1, notes_root, siblings_1);
}
