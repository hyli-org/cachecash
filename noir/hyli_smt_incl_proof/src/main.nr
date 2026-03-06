use dep::poseidon::poseidon2::Poseidon2;
use std::ops::WrappingAdd;

// ---------------------------------------------------------------------------
// SMT hash helpers (Poseidon2-based, single-Field representation)
// ---------------------------------------------------------------------------

// Convert 32 raw bytes to a Field element using the same interpretation as
// the Rust Poseidon2Hasher::write_h256 (treat bytes as LE integer, reduce mod p).
fn bytes32_to_field(bytes: [u8; 32]) -> Field {
    let mut result: Field = 0;
    let mut scale: Field = 1;
    for i in 0..32 {
        result += (bytes[i] as Field) * scale;
        scale *= 256;
    }
    result
}

// Bit at position h (LSB-first within each byte, matching H256::get_bit)
fn get_bit(key: [u8; 32], h: u8) -> u1 {
    let byte_pos: u8 = h / 8;
    let bit_pos: u8 = h % 8;
    ((key[byte_pos as u32] >> bit_pos) & 1) as u1
}

// Poseidon2([height, node_key, value], 3)
fn hash_base_node(height: u8, node_key: Field, value: Field) -> Field {
    Poseidon2::hash([height as Field, node_key, value], 3)
}

// Poseidon2([2, base_node, zero_bits, zero_count], 4)
fn hash_mwz(base_node: Field, zero_bits: Field, zero_count: u8) -> Field {
    Poseidon2::hash([2, base_node, zero_bits, zero_count as Field], 4)
}

// Poseidon2([1, height, node_key, lhs, rhs], 5)
fn hash_merge_normal(height: u8, node_key: Field, lhs: Field, rhs: Field) -> Field {
    Poseidon2::hash([1, height as Field, node_key, lhs, rhs], 5)
}

// ---------------------------------------------------------------------------
// Single-leaf SMT inclusion proof
// ---------------------------------------------------------------------------

fn verify_inclusion(
    commitment_bytes: [u8; 32],
    commitment_field: Field,
    notes_root: Field,
    siblings: [Field; 256],
) {
    // Handle padding note: if commitment is zero, skip verification.
    if commitment_field != 0 {
        // Accumulator state for parent_path computation.
        // parent_path(key, h) = key with bits 0..=h zeroed (as a Field).
        // We accumulate the low bits and subtract from the full field value.
        let mut accumulated: Field = 0;
        let mut power_of_2: Field = 1;

        let mut current_is_mwz: bool = false;
        let mut current_value: Field = commitment_field;
        let mut mwz_base: Field = 0;
        let mut mwz_zero_bits: Field = 0;
        let mut mwz_zero_count: u8 = 0;

        for h in 0..256 {
            let h_u8 = h as u8;
            // Extract bit from raw bytes (matches Rust H256::get_bit)
            let bit = get_bit(commitment_bytes, h_u8);

            // Incrementally compute parent_path as Field
            accumulated += (bit as Field) * power_of_2;
            let node_key = commitment_field - accumulated;

            let sib = siblings[h];

            // Hash of current accumulator
            let current_hash: Field = if current_is_mwz {
                hash_mwz(mwz_base, mwz_zero_bits, mwz_zero_count)
            } else {
                current_value
            };

            if sib == 0 {
                // Merge with zero: current goes into MergeWithZero
                // set_bit = true when current is on the RIGHT (bit==1)
                let set_bit = (bit == 1);
                if current_is_mwz {
                    if set_bit {
                        mwz_zero_bits += power_of_2;
                    }
                    mwz_zero_count = mwz_zero_count.wrapping_add(1);
                } else {
                    mwz_base = hash_base_node(h_u8, node_key, current_value);
                    mwz_zero_bits = 0;
                    if set_bit {
                        mwz_zero_bits = power_of_2;
                    }
                    mwz_zero_count = 1;
                    current_is_mwz = true;
                }
            } else {
                // Normal merge: both sides non-zero
                let lhs: Field = if bit == 0 { current_hash } else { sib };
                let rhs: Field = if bit == 0 { sib } else { current_hash };
                current_value = hash_merge_normal(h_u8, node_key, lhs, rhs);
                current_is_mwz = false;
            }

            power_of_2 *= 2;
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
    // Private SMT proof witnesses - single Field per sibling
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
    // Parse blob: [commitment_0 (32B)][commitment_1 (32B)][notes_root (32B)]
    // -------------------------------------------------------------------------
    let mut commitment_0_bytes: [u8; 32] = [0; 32];
    let mut commitment_1_bytes: [u8; 32] = [0; 32];
    let mut notes_root_bytes: [u8; 32] = [0; 32];

    for i in 0..32 {
        commitment_0_bytes[i] = blob[i];
        commitment_1_bytes[i] = blob[32 + i];
        notes_root_bytes[i] = blob[64 + i];
    }

    let commitment_0 = bytes32_to_field(commitment_0_bytes);
    let commitment_1 = bytes32_to_field(commitment_1_bytes);
    let notes_root = bytes32_to_field(notes_root_bytes);

    // -------------------------------------------------------------------------
    // Verify SMT inclusion for both commitments
    // -------------------------------------------------------------------------
    verify_inclusion(commitment_0_bytes, commitment_0, notes_root, siblings_0);
    verify_inclusion(commitment_1_bytes, commitment_1, notes_root, siblings_1);
}
