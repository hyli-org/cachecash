use common::{
    check_commitment, check_input_note_ownership, get_address, get_note_commitment, InputNote, Note,
};
use poseidon::poseidon2;

pub fn utxo_main(
    input_notes: [InputNote; 2],
    output_notes: [Note; 2],
    pmessage4: Field,
    commitments: [Field; 4],
    messages: [Field; 5],
) {
    // Check the commitments
    check_commitment(input_notes[0].note, commitments[0]);
    check_commitment(input_notes[1].note, commitments[1]);
    check_commitment(output_notes[0], commitments[2]);
    check_commitment(output_notes[1], commitments[3]);

    // Check individual outputs are not greater than 240 bits
    output_notes[0].value.assert_max_bit_size::<240>();
    output_notes[1].value.assert_max_bit_size::<240>();

    let input_value = input_notes[0].note.value + input_notes[1].note.value;

    let output_value = output_notes[0].value + output_notes[1].value;

    // Check for multiple kinds, not currently supported in this proof
    let (is_multiple_kinds, _note_kind) = is_multiple_kinds([
        input_notes[0].note,
        input_notes[1].note,
        output_notes[0],
        output_notes[1],
    ]);
    assert(!is_multiple_kinds, "Inconsistent kinds are not allowed");

    // Kind of UTXO txn
    let kind = messages[0];

    // Messages (only for mint/burns, otherwise all zeros)
    let msg_note_kind = messages[1];
    let msg_value = messages[2];

    // Assert message value size is max 240 bits to prevent overflow attack
    msg_value.assert_max_bit_size::<240>();

    let msg_hash = messages[3];
    let burn_addr = messages[4];

    let mint_hash = poseidon2::Poseidon2::hash([output_notes[0].psi, output_notes[1].psi], 2);

    let burn_hash = commitments[0];

    if (kind == 1) {
        assert(input_value == output_value, "Input and output totals do not match");
    } else if (kind == 2) {
        assert(output_value == input_value + msg_value, "Mint output must match value message");
        assert(mint_hash == msg_hash, "Mint hash must match message");
        assert(output_notes[0].kind == msg_note_kind, "Mint note kind must match message")
    } else if (kind == 3) {
        assert(pmessage4 == burn_addr, "messages[4] must match private input");
        assert(burn_hash == msg_hash, "Burn hash must match message");
        assert(input_value == output_value + msg_value, "Burn output must match value message");
        assert(input_notes[0].note.kind == msg_note_kind, "Burn note kind must match message")
    } else {
        assert(false, "Invalid kind");
    }

    // Check the user owns the input notes
    check_input_note_ownership(input_notes[0]);
    check_input_note_ownership(input_notes[1]);
}

fn is_multiple_kinds(notes: [Note; 4]) -> (bool, Field) {
    let mut first_non_zero_kind = 0;
    let mut is_multiple_kinds = false;

    for i in 0..4 {
        if !is_multiple_kinds {
            if notes[i].kind != 0 {
                if first_non_zero_kind == 0 {
                    first_non_zero_kind = notes[i].kind;
                } else if first_non_zero_kind != notes[i].kind {
                    is_multiple_kinds = true;
                }
            }
        }
    }

    (is_multiple_kinds, first_non_zero_kind)
}

fn bridged_note_kind() -> Field {
    3533694129556768672311144317398675444585744224105014452550528428861358080
}

#[test]
fn test_send_two_inputs_two_outputs() {
    let pk: Field = 101;
    let address = get_address(pk);

    let note_1 = Note { kind: bridged_note_kind(), value: 10, address, psi: 1 };
    let note_2 = Note { kind: bridged_note_kind(), value: 5, address, psi: 2 };
    let input_note_1 = InputNote { note: note_1, secret_key: pk };
    let input_note_2 = InputNote { note: note_2, secret_key: pk };

    let note_3 = Note { kind: bridged_note_kind(), value: 1, address, psi: 3 };
    let note_4 = Note { kind: bridged_note_kind(), value: 14, address, psi: 4 };

    utxo_main(
        [input_note_1, input_note_2],
        [note_3, note_4],
        0,
        [
            get_note_commitment(note_1),
            get_note_commitment(note_2),
            get_note_commitment(note_3),
            get_note_commitment(note_4),
        ],
        [1, 0, 0, 0, 0],
    )
}

#[test]
fn test_send_one_input_two_outputs() {
    let pk: Field = 101;
    let address = get_address(pk);

    let note_1 = Note { kind: bridged_note_kind(), value: 10, address, psi: 1 };
    let note_2 = Note { kind: 0, value: 0, address, psi: 0 };
    let input_note_1 = InputNote { note: note_1, secret_key: pk };
    let input_note_2 = InputNote { note: note_2, secret_key: 0 };

    let note_3 = Note { kind: bridged_note_kind(), value: 1, address, psi: 3 };
    let note_4 = Note { kind: bridged_note_kind(), value: 9, address, psi: 4 };

    utxo_main(
        [input_note_1, input_note_2],
        [note_3, note_4],
        0,
        [
            get_note_commitment(note_1),
            get_note_commitment(note_2),
            get_note_commitment(note_3),
            get_note_commitment(note_4),
        ],
        [1, 0, 0, 0, 0],
    )
}

#[test]
fn test_send_two_inputs_one_output() {
    let pk: Field = 101;
    let address = get_address(pk);

    let note_1 = Note { kind: bridged_note_kind(), value: 10, address, psi: 1 };
    let note_2 = Note { kind: bridged_note_kind(), value: 5, address, psi: 2 };
    let input_note_1 = InputNote { note: note_1, secret_key: pk };
    let input_note_2 = InputNote { note: note_2, secret_key: pk };

    let note_3 = Note { kind: bridged_note_kind(), value: 15, address, psi: 3 };
    let note_4 = Note { kind: 0, value: 0, address, psi: 0 };

    utxo_main(
        [input_note_1, input_note_2],
        [note_3, note_4],
        0,
        [
            get_note_commitment(note_1),
            get_note_commitment(note_2),
            get_note_commitment(note_3),
            get_note_commitment(note_4),
        ],
        [1, 0, 0, 0, 0],
    )
}

#[test(should_fail)]
fn test_send_diff_kinds() {
    let pk: Field = 101;
    let address = get_address(pk);

    let note_1 = Note { kind: bridged_note_kind(), value: 15, address, psi: 1 };
    let note_2 = Note { kind: 2, value: 5, address, psi: 2 };
    let input_note_1 = InputNote { note: note_1, secret_key: pk };
    let input_note_2 = InputNote { note: note_2, secret_key: pk };

    let note_3 = Note { kind: bridged_note_kind(), value: 15, address, psi: 3 };
    let note_4 = Note { kind: 2, value: 5, address, psi: 4 };

    utxo_main(
        [input_note_1, input_note_2],
        [note_3, note_4],
        0,
        [
            get_note_commitment(note_1),
            get_note_commitment(note_2),
            get_note_commitment(note_3),
            get_note_commitment(note_4),
        ],
        [1, 0, 0, 0, 0],
    )
}

#[test]
fn test_mint() {
    let pk: Field = 101;
    let address = get_address(pk);

    let note_1 = Note { kind: bridged_note_kind(), value: 15, address, psi: 1 };
    let note_2 = Note { kind: bridged_note_kind(), value: 5, address, psi: 2 };
    let input_note_1 = InputNote { note: note_1, secret_key: pk };
    let input_note_2 = InputNote { note: note_2, secret_key: pk };

    let note_3 = Note { kind: bridged_note_kind(), value: 0, address: 0, psi: 0 };
    let note_4 = Note { kind: bridged_note_kind(), value: 30, address, psi: 3 };

    let commitments = [
        get_note_commitment(note_1),
        get_note_commitment(note_2),
        get_note_commitment(note_3),
        get_note_commitment(note_4),
    ];

    let mint_hash = poseidon2::Poseidon2::hash([note_3.psi, note_4.psi], 2);

    utxo_main(
        [input_note_1, input_note_2],
        [note_3, note_4],
        0,
        commitments,
        [2, bridged_note_kind(), 10, mint_hash, 0],
    )
}

#[test]
fn test_burn() {
    let pk: Field = 101;
    let address = get_address(pk);

    let note_1 = Note { kind: bridged_note_kind(), value: 15, address, psi: 1 };
    let note_2 = Note { kind: bridged_note_kind(), value: 5, address, psi: 2 };
    let input_note_1 = InputNote { note: note_1, secret_key: pk };
    let input_note_2 = InputNote { note: note_2, secret_key: pk };

    let note_3 = Note { kind: 0, value: 0, address: 0, psi: 0 };
    let note_4 = Note { kind: bridged_note_kind(), value: 10, address, psi: 3 };

    utxo_main(
        [input_note_1, input_note_2],
        [note_3, note_4],
        0,
        [
            get_note_commitment(note_1),
            get_note_commitment(note_2),
            get_note_commitment(note_3),
            get_note_commitment(note_4),
        ],
        [3, bridged_note_kind(), 10, get_note_commitment(note_1), 0],
    )
}

#[test]
fn test_send_zero_value() {
    let pk: Field = 101;
    let address = get_address(pk);

    let note_1 = Note { kind: bridged_note_kind(), value: 0, address, psi: 1 };
    let note_2 = Note { kind: 0, value: 0, address, psi: 0 };
    let input_note_1 = InputNote { note: note_1, secret_key: pk };
    let input_note_2 = InputNote { note: note_2, secret_key: 0 };

    let note_3 = Note { kind: bridged_note_kind(), value: 0, address, psi: 3 };
    let note_4 = Note { kind: 0, value: 0, address, psi: 0 };

    utxo_main(
        [input_note_1, input_note_2],
        [note_3, note_4],
        0,
        [
            get_note_commitment(note_1),
            get_note_commitment(note_2),
            get_note_commitment(note_3),
            get_note_commitment(note_4),
        ],
        [1, 0, 0, 0, 0],
    )
}

#[test]
fn test_mint_with_padding_input() {
    let pk: Field = 101;
    let address = get_address(pk);

    let note_1 = Note { kind: bridged_note_kind(), value: 5, address, psi: 1 };
    let note_2 = Note { kind: 0, value: 0, address, psi: 0 };
    let input_note_1 = InputNote { note: note_1, secret_key: pk };
    let input_note_2 = InputNote { note: note_2, secret_key: 0 };

    let note_3 = Note { kind: bridged_note_kind(), value: 15, address, psi: 3 };
    let note_4 = Note { kind: bridged_note_kind(), value: 0, address: 0, psi: 0 };

    let commitments = [
        get_note_commitment(note_1),
        get_note_commitment(note_2),
        get_note_commitment(note_3),
        get_note_commitment(note_4),
    ];

    let mint_hash = poseidon2::Poseidon2::hash([note_3.psi, note_4.psi], 2);

    utxo_main(
        [input_note_1, input_note_2],
        [note_3, note_4],
        0,
        commitments,
        [2, bridged_note_kind(), 10, mint_hash, 0],
    )
}

#[test]
fn test_burn_single_input() {
    let pk: Field = 101;
    let address = get_address(pk);

    let note_1 = Note { kind: bridged_note_kind(), value: 15, address, psi: 1 };
    let note_2 = Note { kind: 0, value: 0, address, psi: 0 };
    let input_note_1 = InputNote { note: note_1, secret_key: pk };
    let input_note_2 = InputNote { note: note_2, secret_key: 0 };

    let note_3 = Note { kind: 0, value: 0, address: 0, psi: 0 };
    let note_4 = Note { kind: bridged_note_kind(), value: 5, address, psi: 3 };

    utxo_main(
        [input_note_1, input_note_2],
        [note_3, note_4],
        0,
        [
            get_note_commitment(note_1),
            get_note_commitment(note_2),
            get_note_commitment(note_3),
            get_note_commitment(note_4),
        ],
        [3, bridged_note_kind(), 10, get_note_commitment(note_1), 0],
    )
}
