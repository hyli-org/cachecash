use dep::poseidon::poseidon2::Poseidon2;
use common::{compute_nullifier, get_note_commitment, InputNote};
use hyli_noir_sdk::{HyliOutput, le_bytes_to_field, parse_structured_blob};
use std::ops::WrappingAdd;

global BLOB_SLOTS: u32 = 1;
global INITIAL_STATE_MAX: u32 = 4;
global NEXT_STATE_MAX: u32 = 4;
global IDENTITY_MAX: u32 = 256;
global BLOB_NAME_MAX: u32 = 256;
global BLOB_DATA_MAX: u32 = 110;
global PROGRAM_OUTPUT_MAX: u32 = 5;
global SMT_PAYLOAD_LEN: u32 = 96;

// ---------------------------------------------------------------------------
// SMT hash helpers (Poseidon2-based, ZK-native)
// ---------------------------------------------------------------------------

// Poseidon2([height, nk, v], 3) -- nk is byte-pattern (needs conversion), v is Field
fn hash_base_node(height: u8, node_key: [u8; 32], value: Field) -> Field {
    let nk = le_bytes_to_field(node_key);
    Poseidon2::hash([height as Field, nk, value], 3)
}

// Poseidon2([2, bn, zb, zero_count], 4) -- bn is Field, zb is byte-pattern
fn hash_mwz(base_node: Field, zero_bits: [u8; 32], zero_count: u8) -> Field {
    let zb = le_bytes_to_field(zero_bits);
    Poseidon2::hash([2, base_node, zb, zero_count as Field], 4)
}

// Poseidon2([1, height, nk, l, r], 5) -- nk is byte-pattern, l/r are Fields
fn hash_merge_normal(height: u8, node_key: [u8; 32], lhs: Field, rhs: Field) -> Field {
    let nk = le_bytes_to_field(node_key);
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

fn verify_inclusion(commitment_field: Field, notes_root: Field, siblings: [Field; 256]) {
    // Handle padding note: if commitment is zero, skip verification.
    if commitment_field != 0 {
        // Get BE bytes matching the Rust SMT key storage: BorshableH256::from(field.to_be_bytes())
        let commitment_bytes: [u8; 32] = commitment_field.to_be_bytes();

        // The SMT hasher sees these bytes via write_h256 which interprets them as LE integer.
        let commitment_smt_value = le_bytes_to_field(commitment_bytes);

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
    hyli_output: pub HyliOutput<INITIAL_STATE_MAX, NEXT_STATE_MAX, IDENTITY_MAX, BLOB_SLOTS, BLOB_NAME_MAX, BLOB_DATA_MAX, PROGRAM_OUTPUT_MAX>,
    // Private inputs: input notes (note data + secret key for nullifier computation)
    input_notes: [InputNote; 2],
    // Private SMT proof witnesses
    siblings_0: [Field; 256],
    siblings_1: [Field; 256],
) {
    // -------------------------------------------------------------------------
    // Hyli contract framing
    // -------------------------------------------------------------------------
    hyli_output.verify(1, 4, 4);

    let blob_input = hyli_output.blobs[0];
    blob_input.verify(19, BLOB_DATA_MAX, hyli_output.index);

    let _ = hyli_output.initial_state;
    let _ = hyli_output.next_state;
    let _ = hyli_output.identity;
    let _ = hyli_output.tx_hash;
    let _ = blob_input.contract_name;
    let _ = hyli_output.program_outputs;

    // -------------------------------------------------------------------------
    // Parse structured blob:
    // - caller = Some(BlobIndex(0))
    // - callees = None
    // - parameters = [nullifier_0 (32B)][nullifier_1 (32B)][notes_root (32B)]
    // -------------------------------------------------------------------------
    let blob = parse_structured_blob(blob_input.data);
    assert(blob.caller_present, "caller must be present");
    assert(blob.caller_index == 0, "caller blob index must be zero");
    assert(!blob.callees_present, "callees must be absent");
    assert(blob.parameters_len == SMT_PAYLOAD_LEN, "payload length must be 96 bytes");

    let blob_nullifier_0 = blob.read_bytes32(0);
    let blob_nullifier_1 = blob.read_bytes32(32);
    let notes_root = blob.read_field_le(64);

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
