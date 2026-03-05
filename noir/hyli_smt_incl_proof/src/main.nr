use dep::poseidon::poseidon2::Poseidon2;
use common::{compute_nullifier, get_note_commitment, InputNote};
use std::ops::WrappingAdd;

/// Convert a commitment Field to its big-endian byte representation.
fn commitment_to_bytes(commitment: Field) -> [u8; 32] {
    if commitment == 0 {
        [0u8; 32]
    } else {
        commitment.to_be_bytes()
    }
}

// ---------------------------------------------------------------------------
// SMT hash helpers (Poseidon2-based, ZK-native)
// ---------------------------------------------------------------------------

// Split a 32-byte LE H256 into two 128-bit Field elements (lo, hi).
// Byte 0 is LSB. lo = bytes[0..16], hi = bytes[16..32].
fn h256_to_fields(input: [u8; 32]) -> (Field, Field) {
    let mut lo: Field = 0;
    let mut hi: Field = 0;
    let mut scale: Field = 1;
    for i in 0..16 {
        lo += (input[i] as Field) * scale;
        hi += (input[16 + i] as Field) * scale;
        scale *= 256;
    }
    (lo, hi)
}

// Convert a Field back to a 32-byte LE H256.
// Field.to_be_bytes gives big-endian; reverse for little-endian.
fn field_to_h256(f: Field) -> [u8; 32] {
    let be: [u8; 32] = f.to_be_bytes();
    let mut le: [u8; 32] = [0; 32];
    for i in 0..32 {
        le[i] = be[31 - i];
    }
    le
}

// Poseidon2([height, nk_lo, nk_hi, v_lo, v_hi], 5)
fn hash_base_node(height: u8, node_key: [u8; 32], value: [u8; 32]) -> [u8; 32] {
    let (nk_lo, nk_hi) = h256_to_fields(node_key);
    let (v_lo, v_hi) = h256_to_fields(value);
    let h = Poseidon2::hash([height as Field, nk_lo, nk_hi, v_lo, v_hi], 5);
    field_to_h256(h)
}

// Poseidon2([2, bn_lo, bn_hi, zb_lo, zb_hi, zero_count], 6)
fn hash_mwz(base_node: [u8; 32], zero_bits: [u8; 32], zero_count: u8) -> [u8; 32] {
    let (bn_lo, bn_hi) = h256_to_fields(base_node);
    let (zb_lo, zb_hi) = h256_to_fields(zero_bits);
    let h = Poseidon2::hash([2, bn_lo, bn_hi, zb_lo, zb_hi, zero_count as Field], 6);
    field_to_h256(h)
}

// Poseidon2([1, height, nk_lo, nk_hi, l_lo, l_hi, r_lo, r_hi], 8)
fn hash_merge_normal(height: u8, node_key: [u8; 32], lhs: [u8; 32], rhs: [u8; 32]) -> [u8; 32] {
    let (nk_lo, nk_hi) = h256_to_fields(node_key);
    let (l_lo, l_hi) = h256_to_fields(lhs);
    let (r_lo, r_hi) = h256_to_fields(rhs);
    let h = Poseidon2::hash(
        [1, height as Field, nk_lo, nk_hi, l_lo, l_hi, r_lo, r_hi],
        8,
    );
    field_to_h256(h)
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

fn verify_inclusion(commitment: [u8; 32], notes_root: [u8; 32], siblings: [[u8; 32]; 256]) {
    // Handle padding note: if commitment is zero, skip verification.
    let mut commitment_is_zero = true;
    for i in 0..32 {
        if commitment[i] != 0 {
            commitment_is_zero = false;
        }
    }

    if !commitment_is_zero {
        // Accumulator state
        let mut current_is_mwz: bool = false;
        let mut current_value: [u8; 32] = commitment; // Value case
        let mut mwz_base: [u8; 32] = [0; 32];
        let mut mwz_zero_bits: [u8; 32] = [0; 32];
        let mut mwz_zero_count: u8 = 0;

        for h in 0..256 {
            let h_u8 = h as u8;
            let node_key = parent_path(commitment, h_u8);
            let bit = get_bit(commitment, h_u8);
            let sib = siblings[h];

            // Is sibling zero?
            let mut sib_is_zero = true;
            for i in 0..32 {
                if sib[i] != 0 {
                    sib_is_zero = false;
                }
            }

            // Hash of current accumulator
            let current_hash: [u8; 32] = if current_is_mwz {
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
                // Normal merge: both sides non-zero
                let lhs: [u8; 32] = if bit == 0 { current_hash } else { sib };
                let rhs: [u8; 32] = if bit == 0 { sib } else { current_hash };
                current_value = hash_merge_normal(h_u8, node_key, lhs, rhs);
                current_is_mwz = false;
            }
        }

        // Final computed root
        let computed_root: [u8; 32] = if current_is_mwz {
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
    siblings_0: [[u8; 32]; 256],
    siblings_1: [[u8; 32]; 256],
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
    let mut notes_root: [u8; 32] = [0; 32];

    for i in 0..32 {
        blob_nullifier_0[i] = blob[i];
        blob_nullifier_1[i] = blob[32 + i];
        notes_root[i] = blob[64 + i];
    }

    // -------------------------------------------------------------------------
    // Compute commitments (for SMT lookup) and nullifiers (for blob verification)
    // -------------------------------------------------------------------------
    let commitment_0 = commitment_to_bytes(get_note_commitment(input_notes[0].note));
    let commitment_1 = commitment_to_bytes(get_note_commitment(input_notes[1].note));

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
