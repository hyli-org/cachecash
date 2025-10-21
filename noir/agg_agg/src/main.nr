use dep::std;
use poseidon::poseidon2;

global AGG_UTXO_VERIFICATION_KEY_HASH: Field =
    17312275849526493518721480386830253666706187309474064656261199404280431862138;

struct UtxoAggProof {
    proof: [Field; 508],
    old_root: Field,
    new_root: Field,
    commit_hash: Field,
}

fn main(
    verification_key: [Field; 115],
    verification_key_hash: Field,
    utxo_agg_proofs: [UtxoAggProof; 2],
    old_root: pub Field,
    new_root: pub Field,
    commit_hash: pub Field,
    // 6 proofs, 6 per proof
    messages: pub [Field; 30],
) {
    // Hardcode verification_key_hash for agg_utxo proof
    assert(verification_key_hash == AGG_UTXO_VERIFICATION_KEY_HASH, "only agg_utxo proof allowed");

    let mut root = old_root;
    let mut first_public_inputs: [Field; 18] = [0; 18];

    for i in 0..2 {
        let proof = utxo_agg_proofs[i];
        let public_inputs = [
            messages[15 * i],
            messages[15 * i + 1],
            messages[15 * i + 2],
            messages[15 * i + 3],
            messages[15 * i + 4],
            messages[15 * i + 5],
            messages[15 * i + 6],
            messages[15 * i + 7],
            messages[15 * i + 8],
            messages[15 * i + 9],
            messages[15 * i + 10],
            messages[15 * i + 11],
            messages[15 * i + 12],
            messages[15 * i + 13],
            messages[15 * i + 14],
            proof.old_root,
            proof.new_root,
            proof.commit_hash,
        ];

        if i == 0 {
            first_public_inputs = public_inputs;
        }

        std::verify_proof_with_type(
            verification_key,
            if proof.old_root != 0 {
                proof.proof
            } else {
                utxo_agg_proofs[0].proof
            },
            if proof.old_root != 0 {
                public_inputs
            } else {
                first_public_inputs
            },
            verification_key_hash,
            7,
        );

        if proof.old_root != 0 {
            assert(proof.old_root == root, "proof old_root must match previous root");
            root = proof.new_root;
        }
    }

    assert(
        commit_hash
            == poseidon2::Poseidon2::hash(
                [utxo_agg_proofs[0].commit_hash, utxo_agg_proofs[1].commit_hash],
                2,
            ),
    );
    assert(new_root == root, "Roots must match");
}
