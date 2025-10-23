use common::{check_commitment, check_input_note_ownership, InputNote, Note};
use poseidon::poseidon2;
use utxo_lib::utxo_main;

fn field_to_bytes(value: Field) -> [u8; 32] {
    value.to_be_bytes()
}


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
    blob: pub [u8; 128],
    tx_blob_count: pub u32,
    success: pub bool,
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
    assert(success, "contract execution must succeed");
    assert(version == 1, "unsupported Hyli output version");

    assert(initial_state_len == 4, "initial_state must be 4 bytes");
    assert(next_state_len == 4, "next_state must be 4 bytes");

    assert(blob_capacity == 128, "blob capacity must be 128 bytes");
    assert(blob_len == 128, "blob length must be 128 bytes");
    assert(blob_number == 1, "exactly one blob is expected");
    assert(tx_blob_count >= blob_number, "transaction must provide enough blobs");
    assert(blob_index == index, "blob index must align with invocation index");

    // // Basic hygiene on the identity payload so the plumbing is exercised.
    // let identity_bytes = identity.as_bytes();
    // assert(identity_bytes.len() == identity_len as u32, "identity_len mismatch");

    // let tx_hash_bytes = tx_hash.as_bytes();
    // assert(tx_hash_bytes.len() <= 64, "tx hash too long");

    assert(identity_len as u32 <= 256, "identity length overflow");
    assert(blob_contract_name_len == 9, "blob contract name must be 9 bytes");

    let _ = initial_state;
    let _ = next_state;


    // ---------------------------------------------------------------------
    // Call through to the original UTXO circuit
    // ---------------------------------------------------------------------
    utxo_main(
        input_notes,
        output_notes,
        pmessage4,
        commitments,
        messages,
    );

    // Mirror key checks so the Hyli host can rely on them directly.
    check_commitment(input_notes[0].note, commitments[0]);
    check_commitment(input_notes[1].note, commitments[1]);
    check_commitment(output_notes[0], commitments[2]);
    check_commitment(output_notes[1], commitments[3]);

    check_input_note_ownership(input_notes[0]);
    check_input_note_ownership(input_notes[1]);

    // ---------------------------------------------------------------------
    // Collect nullifier commitments to expose them through the blob payload.
    // ---------------------------------------------------------------------
    let mut nullifier_commitments: [Field; 2] = [0; 2];
    for i in 0..2 {
        let note = input_notes[i];
        let private_commitment = poseidon2::Poseidon2::hash([note.note.psi, note.secret_key], 2);
        nullifier_commitments[i] = private_commitment;
    }

    let expected_blob = build_blob_payload(commitments, nullifier_commitments);
    assert(blob == expected_blob, "blob must match concatenated commitments");
}

fn build_blob_payload(
    commitments: [Field; 4],
    nullifier_commitments: [Field; 2],
) -> [u8; 128] {
    let mut bytes: [u8; 128] = [0; 128];
    let mut offset: u32 = 0;

    for i in 0..2 {
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
