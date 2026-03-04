use common::{check_commitment, check_input_note_ownership, InputNote, Note};
use utxo_lib::utxo_main;

global ACTION_CREATE_NOTE_FROM_TOKEN_TRANSFER: u8 = 1;
global TOKEN_CONTRACT_NAME_LEN: u8 = 9;
global TOKEN_ACTION_MAX_BYTES: u32 = 256;
global TOKEN_BLOB_CAPACITY: u32 = 256;
global EXPECTED_RECIPIENT_LEN: u32 = 5;

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
    // `parse_noir_output` calls `parse_blobs`, which expects:
    // `blob_number`, then N blob tuples:
    //   (index, contract_name_len, contract_name, capacity, len, data)
    // This first tuple is the hyli_utxo blob.
    blob_index: pub u32,
    blob_contract_name_len: pub u8,
    blob_contract_name: pub str<256>,
    blob_capacity: pub u32,
    blob_len: pub u32,
    blob: pub [u8; 128],
    // Second tuple for `parse_blobs`: smt-token transfer evidence blob.
    // Keeping it in this blob tuple format is what makes it node-verified.
    token_blob_index: pub u32,
    token_blob_contract_name_len: pub u8,
    token_blob_contract_name: pub str<256>,
    token_blob_capacity: pub u32,
    token_blob_len: pub u32,
    token_blob: pub [u8; 256],
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
    // We expose exactly 2 blob tuples to match the public inputs encoded for
    // `parse_blobs`: hyli_utxo blob + smt-token blob.
    assert(blob_number == 2, "exactly two blobs are expected");
    assert(tx_blob_count >= blob_number, "transaction must provide enough blobs");
    assert(blob_index == index, "blob index must align with invocation index");

    // // Basic hygiene on the identity payload so the plumbing is exercised.
    // let identity_bytes = identity.as_bytes();
    // assert(identity_bytes.len() == identity_len as u32, "identity_len mismatch");

    // let tx_hash_bytes = tx_hash.as_bytes();
    // assert(tx_hash_bytes.len() <= 64, "tx hash too long");

    assert(identity_len as u32 <= 256, "identity length overflow");
    assert(blob_contract_name_len == 9, "blob contract name must be 9 bytes");
    assert(
        token_blob_contract_name_len == TOKEN_CONTRACT_NAME_LEN,
        "invalid token blob contract name len",
    );
    assert(token_blob_capacity == TOKEN_BLOB_CAPACITY, "invalid token blob capacity");
    assert(token_blob_len > 0, "token blob payload must not be empty");
    assert(token_blob_len <= TOKEN_ACTION_MAX_BYTES, "token blob payload too long");

    let token_contract_name_bytes = token_blob_contract_name.as_bytes();
    assert(token_contract_name_bytes[0] == 115, "token contract must be smt-token");
    assert(token_contract_name_bytes[1] == 109, "token contract must be smt-token");
    assert(token_contract_name_bytes[2] == 116, "token contract must be smt-token");
    assert(token_contract_name_bytes[3] == 45, "token contract must be smt-token");
    assert(token_contract_name_bytes[4] == 116, "token contract must be smt-token");
    assert(token_contract_name_bytes[5] == 111, "token contract must be smt-token");
    assert(token_contract_name_bytes[6] == 107, "token contract must be smt-token");
    assert(token_contract_name_bytes[7] == 101, "token contract must be smt-token");
    assert(token_contract_name_bytes[8] == 110, "token contract must be smt-token");

    // Ensure the second tuple is a real tx blob reference and not the same
    // tuple as hyli_utxo.
    assert(token_blob_index < tx_blob_count, "token blob index out of range");
    assert(token_blob_index != blob_index, "token blob must differ from hyli_utxo blob");
    assert(ACTION_CREATE_NOTE_FROM_TOKEN_TRANSFER == 1, "action constant mismatch");

    let _ = initial_state;
    let _ = next_state;
    let _ = blob_contract_name;
    let _ = identity;
    let _ = tx_hash;
    let _ = blob;

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

    let transfer_amount = parse_and_validate_transfer_action(token_blob, token_blob_len);
    assert(output_notes[0].value == transfer_amount, "created note amount must match transfer");
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
    assert(recipient_len == EXPECTED_RECIPIENT_LEN, "recipient must be id@id");

    let recipient_start = sender_end + 4;
    assert(recipient_start + EXPECTED_RECIPIENT_LEN + 16 <= token_action_len, "invalid recipient");

    assert(token_action[recipient_start] == 105, "recipient must be id@id");
    assert(token_action[recipient_start + 1] == 100, "recipient must be id@id");
    assert(token_action[recipient_start + 2] == 64, "recipient must be id@id");
    assert(token_action[recipient_start + 3] == 105, "recipient must be id@id");
    assert(token_action[recipient_start + 4] == 100, "recipient must be id@id");

    let amount_start = recipient_start + EXPECTED_RECIPIENT_LEN;
    assert(amount_start + 16 == token_action_len, "unexpected trailing token action bytes");

    read_u128_le_as_field(token_action, amount_start)
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
