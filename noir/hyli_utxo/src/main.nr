use common::{check_commitment, check_input_note_ownership, InputNote, Note};
use utxo_lib::utxo_main;

global HYLI_OUTPUT_VERSION: u32 = 2;
global HYLI_BLOB_SLOTS: u32 = 2;
global HYLI_BLOB_NAME_MAX: u32 = 256;
global HYLI_BLOB_DATA_MAX: u32 = 256;
global HYLI_PROGRAM_OUTPUTS_MAX: u32 = 4;
global HYLI_UTXO_CONTRACT_NAME_LEN: u32 = 9;
global HYLI_UTXO_BLOB_LEN: u32 = 128;
global TOKEN_CONTRACT_NAME_LEN: u32 = 9;
global TOKEN_ACTION_MAX_BYTES: u32 = 256;
global EXPECTED_RECIPIENT_LEN: u32 = 5;

struct BlobInput<let NAME_MAX: u32, let DATA_MAX: u32> {
    index: u32,
    contract_name_len: u32,
    contract_name: str<NAME_MAX>,
    data_len: u32,
    data: [u8; DATA_MAX],
}

struct HyliOutput<let BLOBS_MAX: u32, let BLOB_NAME_MAX: u32, let BLOB_DATA_MAX: u32, let PROGRAM_OUTPUTS_MAX: u32> {
    version: u32,
    initial_state_len: u32,
    initial_state: [u8; 4],
    next_state_len: u32,
    next_state: [u8; 4],
    identity_len: u32,
    identity: str<256>,
    index: u32,
    blob_count: u32,
    blob_slots: u32,
    blob_name_max: u32,
    blob_data_max: u32,
    blobs: [BlobInput<BLOB_NAME_MAX, BLOB_DATA_MAX>; BLOBS_MAX],
    tx_blob_count: u32,
    tx_hash_len: u32,
    tx_hash: [u8; 32],
    success: bool,
    program_outputs_max: u32,
    program_outputs_len: u32,
    program_outputs: [u8; PROGRAM_OUTPUTS_MAX],
}

fn main(
    hyli_output: pub HyliOutput<HYLI_BLOB_SLOTS, HYLI_BLOB_NAME_MAX, HYLI_BLOB_DATA_MAX, HYLI_PROGRAM_OUTPUTS_MAX>,
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
    assert(hyli_output.version == HYLI_OUTPUT_VERSION, "unsupported Hyli output version");
    assert(hyli_output.initial_state_len == 4, "initial_state must be 4 bytes");
    assert(hyli_output.next_state_len == 4, "next_state must be 4 bytes");
    assert(hyli_output.identity_len <= 256, "identity length overflow");
    assert(hyli_output.blob_slots == HYLI_BLOB_SLOTS, "invalid blob slots");
    assert(hyli_output.blob_name_max == HYLI_BLOB_NAME_MAX, "invalid blob name max");
    assert(hyli_output.blob_data_max == HYLI_BLOB_DATA_MAX, "invalid blob data max");
    assert(hyli_output.blob_count >= 1, "blob_count must be at least 1");
    assert(hyli_output.blob_count <= hyli_output.blob_slots, "blob_count exceeds available slots");
    assert(hyli_output.tx_hash_len == 32, "tx_hash must be 32 bytes");
    assert(
        hyli_output.program_outputs_max == HYLI_PROGRAM_OUTPUTS_MAX,
        "invalid program outputs max",
    );
    assert(
        hyli_output.program_outputs_len <= hyli_output.program_outputs_max,
        "program outputs length overflow",
    );

    let hyli_blob = hyli_output.blobs[0];
    assert(hyli_blob.index == hyli_output.index, "blob index must align with invocation index");
    assert(
        hyli_blob.contract_name_len == HYLI_UTXO_CONTRACT_NAME_LEN,
        "blob contract name must be hyli_utxo",
    );
    assert(hyli_blob.data_len == HYLI_UTXO_BLOB_LEN, "hyli_utxo blob must be 128 bytes");

    assert_contract_name_9(
        hyli_blob.contract_name,
        "hyli_utxo",
        "blob contract must be hyli_utxo",
    );

    // Optional smt-token blob:
    // - when blob_count == 2, run full token checks and enforce transfer amount
    // - when blob_count == 1, skip all token-blob checks
    if hyli_output.blob_count == 2 {
        let token_blob = hyli_output.blobs[1];
        assert(token_blob.index < hyli_output.tx_blob_count, "token blob index out of range");
        assert(token_blob.index != hyli_blob.index, "token blob must differ from hyli_utxo blob");
        assert(
            token_blob.contract_name_len == TOKEN_CONTRACT_NAME_LEN,
            "invalid token blob contract name len",
        );
        assert(token_blob.data_len > 0, "token blob payload must not be empty");
        assert(token_blob.data_len <= TOKEN_ACTION_MAX_BYTES, "token blob payload too long");

        assert_contract_name_9(
            token_blob.contract_name,
            "smt-token",
            "token contract must be smt-token",
        );

        let transfer_amount =
            parse_and_validate_transfer_action(token_blob.data, token_blob.data_len);
        assert(output_notes[0].value == transfer_amount, "created note amount must match transfer");
    }

    let _ = hyli_output.initial_state;
    let _ = hyli_output.next_state;
    let _ = hyli_output.identity;
    let _ = hyli_output.tx_hash;
    let _ = hyli_blob.data;
    let _ = hyli_output.program_outputs;

    // ---------------------------------------------------------------------
    // Call through to the original UTXO circuit
    // ---------------------------------------------------------------------
    utxo_main(input_notes, output_notes, pmessage4, commitments, messages);

    // Mirror key checks so the Hyli host can rely on them directly.
    check_commitment(input_notes[0].note, commitments[0]);
    check_commitment(input_notes[1].note, commitments[1]);
    check_commitment(output_notes[0], commitments[2]);
    check_commitment(output_notes[1], commitments[3]);

    check_input_note_ownership(input_notes[0]);
    check_input_note_ownership(input_notes[1]);
}

fn read_u32_le(bytes: [u8; 256], start: u32) -> u32 {
    assert(start + 4 <= 256, "u32 read out of bounds");
    let mut result: u32 = 0;
    let mut scale: u32 = 1;
    for i in 0..4 {
        let index = start + i as u32;
        result += (bytes[index] as u32) * scale;
        if i < 3 {
            scale *= 256;
        }
    }
    result
}

fn read_u128_le_as_field(bytes: [u8; 256], start: u32) -> Field {
    assert(start + 16 <= 256, "u128 read out of bounds");
    let mut result: Field = 0;
    let mut scale: Field = 1;
    for i in 0..16 {
        let index = start + i as u32;
        result += (bytes[index] as Field) * scale;
        scale *= 256;
    }
    result
}

fn parse_and_validate_transfer_action(token_action: [u8; 256], token_action_len: u32) -> Field {
    assert(token_action_len >= 1 + 4 + 4 + 16, "token action payload too short");
    assert(token_action[0] == 0, "token action must be Transfer");

    let sender_len = read_u32_le(token_action, 1);
    let sender_end = 5 + sender_len;
    assert(sender_end + 4 + EXPECTED_RECIPIENT_LEN + 16 <= token_action_len, "invalid sender len");

    let recipient_len = read_u32_le(token_action, sender_end);
    assert(recipient_len == EXPECTED_RECIPIENT_LEN, "recipient must be bank@hyli-utxo-state");

    let recipient_start = sender_end + 4;
    assert(recipient_start + EXPECTED_RECIPIENT_LEN + 16 <= token_action_len, "invalid recipient");

    assert(token_action[recipient_start] == 105, "recipient must be bank@hyli-utxo-state");
    assert(token_action[recipient_start + 1] == 100, "recipient must be bank@hyli-utxo-state");
    assert(token_action[recipient_start + 2] == 64, "recipient must be bank@hyli-utxo-state");
    assert(token_action[recipient_start + 3] == 105, "recipient must be bank@hyli-utxo-state");
    assert(token_action[recipient_start + 4] == 100, "recipient must be bank@hyli-utxo-state");

    let amount_start = recipient_start + EXPECTED_RECIPIENT_LEN;
    assert(amount_start + 16 == token_action_len, "unexpected trailing token action bytes");

    read_u128_le_as_field(token_action, amount_start)
}

fn assert_contract_name_9<let ERR_LEN: u32>(name: str<256>, expected: str<9>, err: str<ERR_LEN>) {
    let name_bytes = name.as_bytes();
    let expected_bytes = expected.as_bytes();
    for i in 0..9 {
        let idx = i as u32;
        assert(name_bytes[idx] == expected_bytes[idx], err);
    }
}

#[test]
fn test_parse_transfer_action_succeeds() {
    let mut action: [u8; 256] = [0; 256];
    action[0] = 0;
    action[1] = 1;
    action[5] = 120;
    action[6] = 5;
    action[10] = 105;
    action[11] = 100;
    action[12] = 64;
    action[13] = 105;
    action[14] = 100;
    action[15] = 42;
    let amount = parse_and_validate_transfer_action(action, 31);
    assert(amount == 42, "amount should decode from u128 LE bytes");
}
