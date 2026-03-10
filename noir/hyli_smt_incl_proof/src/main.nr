use dep::poseidon::poseidon2::Poseidon2;
use common::{compute_nullifier, get_note_commitment, InputNote};
use hyli_noir_sdk::HyliOutput;
use std::ops::WrappingAdd;

global BLOB_SLOTS: u32 = 1;
global INITIAL_STATE_MAX: u32 = 4;
global NEXT_STATE_MAX: u32 = 4;
global IDENTITY_MAX: u32 = 256;
global BLOB_NAME_MAX: u32 = 256;
global BLOB_DATA_MAX: u32 = 110;
global PROGRAM_OUTPUT_MAX: u32 = 5;
global STRUCTURED_BLOB_LEN: u32 = 110;
global SMT_PAYLOAD_LEN: u32 = 96;
global STRUCTURED_CALLER_OFFSET: u32 = 1;
global STRUCTURED_CALLEES_TAG_OFFSET: u32 = 9;
global STRUCTURED_PARAMETERS_LEN_OFFSET: u32 = 10;
global STRUCTURED_PARAMETERS_OFFSET: u32 = 14;

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

fn verify_inclusion(commitment_field: Field, notes_root: Field, siblings: [Field; 256]) {
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
    hyli_output: pub HyliOutput<INITIAL_STATE_MAX, NEXT_STATE_MAX, IDENTITY_MAX, BLOB_SLOTS, BLOB_NAME_MAX, BLOB_DATA_MAX, PROGRAM_OUTPUT_MAX>,
    // Private inputs: input notes (note data + secret key for nullifier computation)
    input_notes: [InputNote; 2],
    // Private SMT proof witnesses
    siblings_0: [Field; 256],
    siblings_1: [Field; 256],
) {
    // -------------------------------------------------------------------------
    // Hyli contract framing (same pattern as hyli_utxo)
    // -------------------------------------------------------------------------
    assert(hyli_output.success, "contract execution must succeed");
    assert(hyli_output.version == 2, "unsupported Hyli output version");

    assert(hyli_output.initial_state_len == 4, "initial_state must be 4 bytes");
    assert(hyli_output.next_state_len == 4, "next_state must be 4 bytes");
    assert(hyli_output.initial_state_max == INITIAL_STATE_MAX, "initial_state_max mismatch");
    assert(hyli_output.next_state_max == NEXT_STATE_MAX, "next_state_max mismatch");
    assert(hyli_output.identity_max == IDENTITY_MAX, "identity_max mismatch");

    assert(hyli_output.blob_slots == BLOB_SLOTS, "blob_slots mismatch");
    assert(hyli_output.blob_name_max == BLOB_NAME_MAX, "blob_name_max mismatch");
    assert(hyli_output.blob_data_max == BLOB_DATA_MAX, "blob_data_max mismatch");
    assert(hyli_output.blob_count == 1, "exactly one blob is expected");
    assert(
        hyli_output.blob_count <= hyli_output.tx_blob_count,
        "transaction must provide enough blobs",
    );
    assert(hyli_output.program_outputs_max == PROGRAM_OUTPUT_MAX, "program outputs max mismatch");
    assert(
        hyli_output.program_outputs_len <= hyli_output.program_outputs_max,
        "program outputs len overflow",
    );

    assert(hyli_output.identity_len <= 256, "identity length overflow");

    let blob_input = hyli_output.blobs[0];
    assert(blob_input.contract_name_len == 19, "blob contract name must be 19 bytes");
    assert(blob_input.index == hyli_output.index, "blob index must align with invocation index");
    assert(blob_input.data_len == STRUCTURED_BLOB_LEN, "blob length must be 110 bytes");

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
    assert(blob_input.data[0] == 1, "caller must be present");
    for i in 0..8 {
        assert(
            blob_input.data[STRUCTURED_CALLER_OFFSET + i] == 0,
            "caller blob index must be zero",
        );
    }
    assert(blob_input.data[STRUCTURED_CALLEES_TAG_OFFSET] == 0, "callees must be absent");
    assert(
        blob_input.data[STRUCTURED_PARAMETERS_LEN_OFFSET] == SMT_PAYLOAD_LEN as u8,
        "payload length prefix must be 96 bytes",
    );
    for i in 1..4 {
        assert(
            blob_input.data[STRUCTURED_PARAMETERS_LEN_OFFSET + i] == 0,
            "payload length high bytes must be zero",
        );
    }

    let mut blob_nullifier_0: [u8; 32] = [0; 32];
    let mut blob_nullifier_1: [u8; 32] = [0; 32];
    let mut notes_root_bytes: [u8; 32] = [0; 32];

    for i in 0..32 {
        blob_nullifier_0[i] = blob_input.data[STRUCTURED_PARAMETERS_OFFSET + i];
        blob_nullifier_1[i] = blob_input.data[STRUCTURED_PARAMETERS_OFFSET + 32 + i];
        notes_root_bytes[i] = blob_input.data[STRUCTURED_PARAMETERS_OFFSET + 64 + i];
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
