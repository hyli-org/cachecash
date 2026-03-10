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
    assert(
        (hyli_output.blob_count == 1) | (hyli_output.blob_count == 2),
        "blob_count must be 1 or 2",
    );
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
    assert(blob_input.contract_name_len == 9, "blob contract name must be 9 bytes");
    assert(blob_input.index == hyli_output.index, "blob index must align with invocation index");
    if hyli_output.blob_count == 1 {
        assert(blob_input.data_len == 128, "blob length must be 128 bytes");
    } else {
        assert(blob_input.data_len <= 128, "blob length overflow");
    }

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

    if hyli_output.blob_count == 3 {
        let expected_blob = build_blob_payload(commitments, nullifier_commitments);
        assert(blob_input.data == expected_blob, "blob must match concatenated commitments");
    } else if hyli_output.blob_count == 4 {
        // Blob is Borsh-serialized smt-token action.
        let transfer_blob = hyli_output.blobs[3];
        let action = parse_smt_token_action(transfer_blob.data, transfer_blob.data_len);

        // 1) Transfer amount must match UTXO message amount.
        messages[2].assert_max_bit_size::<128>();
        assert(action.amount == (messages[2] as u128), "smt transfer amount must match message");

        // 2) Recipient must be bank@hyli-utxo-state.
        let expected_recipient: [u8; 20] = "bank@hyli-utxo-state".as_bytes();
        assert(action.variant == 0, "smt action must be Transfer");
        assert(action.recipient.len == 20, "recipient length mismatch");
        for i in 0..20 {
            assert(
                action.recipient.bytes[i] == expected_recipient[i],
                "recipient must be bank@hyli-utxo-state",
            );
        }

        // 3) Sender must match Hyli transaction identity.
        let identity_bytes = hyli_output.identity.as_bytes();
        assert(
            hyli_output.identity_len <= identity_bytes.len(),
            "identity length exceeds identity bytes",
        );
        assert(action.sender.len == hyli_output.identity_len, "sender length mismatch");
        for i in 0..identity_bytes.len() {
            if i < hyli_output.identity_len {
                assert(action.sender.bytes[i] == identity_bytes[i], "sender must match identity");
            }
        }
    } else {
        assert(false, "unsupported blob count");
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

struct ParsedIdentity {
    len: u32,
    bytes: [u8; IDENTITY_MAX],
}

struct ParsedU32 {
    value: u32,
    offset: u32,
}

struct ParsedU128 {
    value: u128,
    offset: u32,
}

struct ParsedIdentityWithOffset {
    identity: ParsedIdentity,
    offset: u32,
}

// Mirrors Rust enum order in smt-token:
// 0 = Transfer, 1 = TransferFrom, 2 = Approve
struct SmtTokenAction {
    variant: u8,
    sender: ParsedIdentity,
    owner: ParsedIdentity,
    spender: ParsedIdentity,
    recipient: ParsedIdentity,
    amount: u128,
}

fn field_to_bytes(value: Field) -> [u8; 32] {
    value.to_be_bytes()
}

fn parse_u32_le(bytes: [u8; BLOB_DATA_MAX], offset: u32, data_len: u32) -> ParsedU32 {
    assert(offset + 4 <= data_len, "borsh u32 out of bounds");
    let mut value: u32 = 0;
    let mut scale: u32 = 1;
    for i in 0..4 {
        value += (bytes[offset + i] as u32) * scale;
        scale *= 256;
    }
    ParsedU32 { value, offset: offset + 4 }
}

fn parse_u128_le(bytes: [u8; BLOB_DATA_MAX], offset: u32, data_len: u32) -> ParsedU128 {
    assert(offset + 16 <= data_len, "borsh u128 out of bounds");
    let mut value: u128 = 0;
    let mut scale: u128 = 1;
    for i in 0..16 {
        value += (bytes[offset + i] as u128) * scale;
        scale *= 256;
    }
    ParsedU128 { value, offset: offset + 16 }
}

fn parse_identity(
    bytes: [u8; BLOB_DATA_MAX],
    offset: u32,
    data_len: u32,
) -> ParsedIdentityWithOffset {
    let parsed_len = parse_u32_le(bytes, offset, data_len);
    let len = parsed_len.value;
    let mut cursor = parsed_len.offset;
    assert(len <= IDENTITY_MAX, "identity too long");
    assert(cursor + len <= data_len, "identity bytes out of bounds");

    let mut identity_bytes: [u8; IDENTITY_MAX] = [0; IDENTITY_MAX];
    for i in 0..IDENTITY_MAX {
        if i < len {
            identity_bytes[i] = bytes[cursor + i];
        }
    }
    cursor += len;

    ParsedIdentityWithOffset {
        identity: ParsedIdentity { len, bytes: identity_bytes },
        offset: cursor,
    }
}

fn parse_smt_token_action(blob: [u8; BLOB_DATA_MAX], data_len: u32) -> SmtTokenAction {
    assert(data_len >= 1, "empty smt-token action");

    let variant = blob[0];
    let mut cursor: u32 = 1;
    let empty_identity = ParsedIdentity { len: 0, bytes: [0; IDENTITY_MAX] };

    let mut sender = empty_identity;
    let mut owner = empty_identity;
    let mut spender = empty_identity;
    let mut recipient = empty_identity;
    let mut amount: u128 = 0;

    if variant == 0 {
        let parsed_sender = parse_identity(blob, cursor, data_len);
        sender = parsed_sender.identity;
        cursor = parsed_sender.offset;

        let parsed_recipient = parse_identity(blob, cursor, data_len);
        recipient = parsed_recipient.identity;
        cursor = parsed_recipient.offset;

        let parsed_amount = parse_u128_le(blob, cursor, data_len);
        amount = parsed_amount.value;
        cursor = parsed_amount.offset;
    } else if variant == 1 {
        let parsed_owner = parse_identity(blob, cursor, data_len);
        owner = parsed_owner.identity;
        cursor = parsed_owner.offset;

        let parsed_spender = parse_identity(blob, cursor, data_len);
        spender = parsed_spender.identity;
        cursor = parsed_spender.offset;

        let parsed_recipient = parse_identity(blob, cursor, data_len);
        recipient = parsed_recipient.identity;
        cursor = parsed_recipient.offset;

        let parsed_amount = parse_u128_le(blob, cursor, data_len);
        amount = parsed_amount.value;
        cursor = parsed_amount.offset;
    } else if variant == 2 {
        let parsed_owner = parse_identity(blob, cursor, data_len);
        owner = parsed_owner.identity;
        cursor = parsed_owner.offset;

        let parsed_spender = parse_identity(blob, cursor, data_len);
        spender = parsed_spender.identity;
        cursor = parsed_spender.offset;

        let parsed_amount = parse_u128_le(blob, cursor, data_len);
        amount = parsed_amount.value;
        cursor = parsed_amount.offset;
    } else {
        assert(false, "unsupported smt-token action variant");
    }

    assert(cursor == data_len, "borsh action trailing bytes");

    SmtTokenAction { variant, sender, owner, spender, recipient, amount }
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
