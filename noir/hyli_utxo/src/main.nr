use common::{check_commitment, check_input_note_ownership, InputNote, Note};
use poseidon::poseidon2;
use utxo_lib::utxo_main;

fn field_to_bytes(value: Field) -> [u8; 32] {
    value.to_be_bytes()
}

fn field_low_bytes(value: Field) -> [u8; 4] {
    let bytes: [u8; 31] = value.to_be_bytes();
    [bytes[27], bytes[28], bytes[29], bytes[30]]
}

fn hash_roots(notes_root: Field, nullifier_root: Field) -> Field {
    poseidon2::Poseidon2::hash([notes_root, nullifier_root], 2)
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
    notes_root_initial: Field,
    nullifier_root_initial: Field,
    // Wrapped UTXO inputs (mirrors noir/utxo)
    input_notes: [InputNote; 2],
    output_notes: [Note; 2],
    pmessage4: Field,
    commitments: pub [Field; 4],
    messages: pub [Field; 5],
    // Private state inputs required to recompute the tree transition.
    input_merkle_paths: [[Field; 160]; 2],
    output_merkle_paths: [[Field; 160]; 2],
    nullifier_merkle_paths: [[Field; 160]; 2],
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

    // Basic hygiene on the identity payload so the plumbing is exercised.
    let identity_bytes = identity.as_bytes();
    assert(identity_bytes.len() == identity_len as u32, "identity_len mismatch");

    let tx_hash_bytes = tx_hash.as_bytes();
    assert(tx_hash_bytes.len() <= 64, "tx hash too long");

    assert(identity_len as u32 <= 256, "identity length overflow");
    assert(blob_contract_name_len == 9, "blob contract name must be 9 bytes");
    let name_bytes = blob_contract_name.as_bytes();
    let expected_bytes = "hyli_utxo".as_bytes();
    for i in 0..9 {
        assert(name_bytes[i] == expected_bytes[i], "blob contract name must be hyli_utxo");
    }

    let initial_state_digest = hash_roots(notes_root_initial, nullifier_root_initial);
    assert(
        initial_state == field_low_bytes(initial_state_digest),
        "initial_state mismatch",
    );

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
    // Recreate the state transition by updating the Merkle tree.
    // ---------------------------------------------------------------------
    let mut notes_root = notes_root_initial;
    let mut note_update_bits: [[u1; 254]; 4] = [[0; 254]; 4];
    let mut note_update_nodes: [[Field; 161]; 4] = [[0; 161]; 4];
    let mut note_update_count: u32 = 0;

    for i in 0..2 {
        let commitment = commitments[i];
        if commitment != 0 {
            let bits: [u1; 254] = commitment.to_le_bits();
            let path = input_merkle_paths[i];
            let (new_root, new_nodes) = apply_merkle_update(
                notes_root,
                commitment,
                0,
                bits,
                path,
                note_update_bits,
                note_update_nodes,
                note_update_count,
            );
            let idx = note_update_count;
            note_update_bits[idx] = bits;
            note_update_nodes[idx] = new_nodes;
            note_update_count += 1;
            notes_root = new_root;
        }
    }

    for i in 0..2 {
        let commitment = commitments[2 + i];
        if commitment != 0 {
            let bits: [u1; 254] = commitment.to_le_bits();
            let path = output_merkle_paths[i];
            let (new_root, new_nodes) = apply_merkle_update(
                notes_root,
                0,
                commitment,
                bits,
                path,
                note_update_bits,
                note_update_nodes,
                note_update_count,
            );
            let idx = note_update_count;
            note_update_bits[idx] = bits;
            note_update_nodes[idx] = new_nodes;
            note_update_count += 1;
            notes_root = new_root;
        }
    }

    let mut nullifier_root = nullifier_root_initial;
    let mut nullifier_update_bits: [[u1; 254]; 4] = [[0; 254]; 4];
    let mut nullifier_update_nodes: [[Field; 161]; 4] = [[0; 161]; 4];
    let mut nullifier_update_count: u32 = 0;
    let mut nullifier_commitments: [Field; 2] = [0; 2];

    for i in 0..2 {
        let note = input_notes[i];
        let private_commitment =
            poseidon2::Poseidon2::hash([note.note.psi, note.secret_key], 2);
        nullifier_commitments[i] = private_commitment;
        if private_commitment != 0 {
            let bits: [u1; 254] = private_commitment.to_le_bits();
            let path = nullifier_merkle_paths[i];
            let (new_root, new_nodes) = apply_merkle_update(
                nullifier_root,
                0,
                private_commitment,
                bits,
                path,
                nullifier_update_bits,
                nullifier_update_nodes,
                nullifier_update_count,
            );
            let idx = nullifier_update_count;
            nullifier_update_bits[idx] = bits;
            nullifier_update_nodes[idx] = new_nodes;
            nullifier_update_count += 1;
            nullifier_root = new_root;
        }
    }

    let next_state_digest = hash_roots(notes_root, nullifier_root);
    assert(
        next_state == field_low_bytes(next_state_digest),
        "next_state mismatch",
    );

    // ---------------------------------------------------------------------
    // Build the blob payload that Hyli expects: concatenated input and nullifier commitments.
    // ---------------------------------------------------------------------
    let expected_blob = build_blob_payload(commitments, nullifier_commitments);
    assert(blob == expected_blob, "blob must match concatenated commitments");
}

fn get_null_root(bits: [u1; 254], merkle_path: [Field; 160]) -> Field {
    get_merkle_root(0, bits, merkle_path)
}

fn get_merkle_root(leaf: Field, bits: [u1; 254], merkle_path: [Field; 160]) -> Field {
    let mut hash = leaf;
    for i in 0..160 {
        let dir = bits[i];
        let sibling = merkle_path[i];
        if dir == 0 {
            hash = poseidon2::Poseidon2::hash([hash, sibling], 2);
        } else {
            hash = poseidon2::Poseidon2::hash([sibling, hash], 2);
        }
    }
    hash
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

fn adjust_path(
    path: [Field; 160],
    bits: [u1; 254],
    prior_bits: [[u1; 254]; 4],
    prior_nodes: [[Field; 161]; 4],
    prior_count: u32,
) -> [Field; 160] {
    let mut adjusted = path;
    for update_index in 0..4 {
        if update_index < prior_count {
            let idx = update_index;
            let prev_bits = prior_bits[idx];
            let mut divergence = 160;
            for level in 0..160 {
                if divergence == 160 {
                    if prev_bits[level] != bits[level] {
                        divergence = level;
                    }
                }
            }
            if divergence < 160 {
                adjusted[divergence] = prior_nodes[idx][divergence];
            }
        }
    }
    adjusted
}

fn apply_merkle_update(
    root: Field,
    leaf_old: Field,
    leaf_new: Field,
    bits: [u1; 254],
    path: [Field; 160],
    prior_bits: [[u1; 254]; 4],
    prior_nodes: [[Field; 161]; 4],
    prior_count: u32,
) -> (Field, [Field; 161]) {
    let adjusted_path =
        adjust_path(path, bits, prior_bits, prior_nodes, prior_count);

    let mut old_hash = leaf_old;
    let mut new_hash = leaf_new;
    let mut new_nodes: [Field; 161] = [0; 161];
    new_nodes[0] = leaf_new;

    for level in 0..160 {
        let sibling = adjusted_path[level];
        let dir = bits[level];

        let parent_old = if dir == 0 {
            poseidon2::Poseidon2::hash([old_hash, sibling], 2)
        } else {
            poseidon2::Poseidon2::hash([sibling, old_hash], 2)
        };

        let parent_new = if dir == 0 {
            poseidon2::Poseidon2::hash([new_hash, sibling], 2)
        } else {
            poseidon2::Poseidon2::hash([sibling, new_hash], 2)
        };

        old_hash = parent_old;
        new_hash = parent_new;
        new_nodes[level + 1] = parent_new;
    }

    assert(old_hash == root, "Merkle path mismatch");

    (new_hash, new_nodes)
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

#[test]
fn test_notes_tree_transition_replaces_commitment() {
    let input_commitment: Field = 123;
    let input_bits: [u1; 254] = input_commitment.to_le_bits();
    let notes_root_initial = get_merkle_root(input_commitment, input_bits, [0; 160]);
    let notes_root_cleared = get_null_root(input_bits, [0; 160]);

    let output_commitment: Field = 345;
    let output_bits: [u1; 254] = output_commitment.to_le_bits();
    let notes_root_final = get_merkle_root(output_commitment, output_bits, [0; 160]);

    assert(notes_root_initial != notes_root_cleared, "clearing should change root");
    assert(notes_root_final != notes_root_cleared, "reinsert should change root");
}

#[test]
fn test_nullifier_tree_transition_inserts_commitment() {
    let private_commitment: Field = 789;
    let bits: [u1; 254] = private_commitment.to_le_bits();
    let nullifier_root_initial = get_null_root(bits, [0; 160]);
    let nullifier_root_final = get_merkle_root(private_commitment, bits, [0; 160]);

    assert(nullifier_root_initial != nullifier_root_final, "nullifier insert updates root");
}

#[test]
fn test_apply_merkle_update_removal_then_insertion() {
    let mut path: [Field; 160] = [0; 160];
    path[0] = 11;
    path[1] = 22;
    path[2] = 33;
    let bits: [u1; 254] = [0; 254];
    let leaf: Field = 42;
    let root = get_merkle_root(leaf, bits, path);

    let zero_bits: [[u1; 254]; 4] = [[0; 254]; 4];
    let zero_nodes: [[Field; 161]; 4] = [[0; 161]; 4];

    let (removed_root, removal_nodes) =
        apply_merkle_update(root, leaf, 0, bits, path, zero_bits, zero_nodes, 0);
    let expected_removed = get_null_root(bits, path);
    assert(removed_root == expected_removed, "removal root mismatch");

    let mut prior_bits = [[0; 254]; 4];
    let mut prior_nodes = [[0; 161]; 4];
    prior_bits[0] = bits;
    prior_nodes[0] = removal_nodes;

    let (restored_root, _) = apply_merkle_update(
        removed_root,
        0,
        leaf,
        bits,
        path,
        prior_bits,
        prior_nodes,
        1,
    );
    assert(restored_root == root, "restored root mismatch");
}

#[test]
fn test_adjust_path_overwrites_divergent_node() {
    let mut path: [Field; 160] = [0; 160];
    path[5] = 55;
    path[6] = 66;

    let mut prior_bits = [[0; 254]; 4];
    prior_bits[0][5] = 1;

    let mut prior_nodes = [[0; 161]; 4];
    prior_nodes[0][5] = 777;

    let mut bits: [u1; 254] = [0; 254];
    bits[5] = 0;

    let adjusted =
        adjust_path(path, bits, prior_bits, prior_nodes, 1);

    assert(adjusted[5] == 777, "divergent node was not overwritten");
    assert(adjusted[6] == path[6], "non-divergent node must remain unchanged");
}
