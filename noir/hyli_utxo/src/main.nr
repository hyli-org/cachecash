use common::{compute_nullifier, InputNote, Note};
use hyli_noir_sdk::HyliOutput;
use utxo_lib::utxo_main;

global BLOB_SLOTS: u32 = 1;
global INITIAL_STATE_MAX: u32 = 4;
global NEXT_STATE_MAX: u32 = 4;
global IDENTITY_MAX: u32 = 256;
global BLOB_NAME_MAX: u32 = 256;
global BLOB_DATA_MAX: u32 = 128;
global PROGRAM_OUTPUT_MAX: u32 = 5;

fn main(
    hyli_output: pub HyliOutput<INITIAL_STATE_MAX, NEXT_STATE_MAX, IDENTITY_MAX, BLOB_SLOTS, BLOB_NAME_MAX, BLOB_DATA_MAX, PROGRAM_OUTPUT_MAX>,
    // Wrapped UTXO inputs (mirrors noir/utxo)
    input_notes: [InputNote; 2],
    output_notes: [Note; 2],
    pmessage4: Field,
    commitments: pub [Field; 4],
    messages: pub [Field; 5],
) {
    // ---------------------------------------------------------------------
    // Hyli contract framing
    // ---------------------------------------------------------------------
    hyli_output.verify(1, 4, 4);

    let blob_input = hyli_output.blobs[0];
    blob_input.verify(9, 128, hyli_output.index);

    let _ = hyli_output.initial_state;
    let _ = hyli_output.next_state;
    let _ = hyli_output.identity;
    let _ = hyli_output.tx_hash;
    let _ = blob_input.contract_name;
    let _ = hyli_output.program_outputs;

    // ---------------------------------------------------------------------
    // Call through to the original UTXO circuit
    // ---------------------------------------------------------------------
    utxo_main(input_notes, output_notes, pmessage4, commitments, messages);

    // ---------------------------------------------------------------------
    // Collect nullifier commitments to expose them through the blob payload.
    // ---------------------------------------------------------------------
    let mut nullifier_commitments: [Field; 2] = [0; 2];
    for i in 0..2 {
        let note = input_notes[i];
        nullifier_commitments[i] = compute_nullifier(note.note.psi, note.secret_key);
    }

    if (hyli_output.tx_blob_count == 3) | (hyli_output.tx_blob_count == 4) {
        let expected_blob = build_blob_payload(commitments, nullifier_commitments);
        assert(blob_input.data == expected_blob, "blob must match concatenated commitments");
    } else {
        assert(false, "unsupported transaction blob count");
    }
}

fn build_blob_payload(commitments: [Field; 4], nullifier_commitments: [Field; 2]) -> [u8; 128] {
    let mut bytes: [u8; 128] = [0; 128];
    let mut offset: u32 = 0;

    for i in 2..4 {
        let commitment_bytes = field_to_bytes(commitments[i]);
        for j in 0..32 {
            let j_idx: u32 = j as u32;
            let index: u32 = offset + j_idx;
            bytes[index] = commitment_bytes[j_idx];
        }
        offset += 32;
    }

    for i in 0..2 {
        let commitment_bytes = field_to_bytes(nullifier_commitments[i]);
        for j in 0..32 {
            let j_idx: u32 = j as u32;
            let index: u32 = offset + j_idx;
            bytes[index] = commitment_bytes[j_idx];
        }
        offset += 32;
    }

    bytes
}

fn field_to_bytes(value: Field) -> [u8; 32] {
    value.to_be_bytes()
}

#[test]
fn test_build_blob_payload_concatenates_commitments() {
    let commitments: [Field; 4] = [11, 22, 33, 44];
    let nullifiers: [Field; 2] = [55, 66];
    let blob = build_blob_payload(commitments, nullifiers);
    let mut expected: [u8; 128] = [0; 128];

    let arrays = [
        field_to_bytes(commitments[0]),
        field_to_bytes(commitments[1]),
        field_to_bytes(nullifiers[0]),
        field_to_bytes(nullifiers[1]),
    ];

    for i in 0..4 {
        let start = i * 32;
        for j in 0..32 {
            expected[start + j] = arrays[i][j];
        }
    }

    assert(blob == expected, "blob must match concatenated commitment bytes");
}
