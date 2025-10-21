use crate::Mode;
use crate::{BlockFormat, PersistentMerkleTree, Result, types::BlockHeight};
use barretenberg::Verify;
use block_store::BlockStore;
use element::Element;
use node_interface::{ElementData, ElementsVecData, RpcError};
use zk_primitives::UtxoProof;

/// Validate a UTXO txn, we check the following:
/// - The proof is valid
/// - The recent root is recent enough
/// - The input notes are not already spent (not in tree)
/// - The output notes do not already exist (not in tree)
pub fn validate_txn(
    _mode: Mode,
    utxo_proof: &UtxoProof,
    _height: BlockHeight,
    block_store: &BlockStore<BlockFormat>,
    notes_tree: &PersistentMerkleTree,
) -> Result<()> {
    if let Err(_err) = utxo_proof.verify() {
        Err(RpcError::InvalidProof)?;
    }

    let public_inputs = &utxo_proof.public_inputs;

    let [input_0, input_1] = public_inputs.input_commitments;
    if input_0 != Element::ZERO && input_0 == input_1 {
        Err(RpcError::TxnDuplicateInputCommitments(ElementsVecData {
            elements: vec![input_0],
        }))?;
    }

    let [output_0, output_1] = public_inputs.output_commitments;
    if output_0 != Element::ZERO && output_0 == output_1 {
        Err(RpcError::TxnDuplicateOutputCommitments(ElementsVecData {
            elements: vec![output_0],
        }))?;
    }

    // Check if any of the txn inserts are already in the tree
    let tree = notes_tree.tree();

    for leaf in utxo_proof.public_inputs.output_commitments {
        if leaf >= Element::MODULUS {
            Err(RpcError::InvalidElementSize(ElementData { element: leaf }))?;
        }

        if leaf != Element::ZERO {
            if tree.contains_element(&leaf) {
                Err(RpcError::TxnOutputCommitmentsExist(ElementsVecData {
                    elements: vec![leaf],
                }))?;
            }

            let (_, output_history) = block_store.get_element_history(leaf)?;
            if output_history.is_some() {
                // This note used to be in tree, but was removed (used as insert in txn)
                Err(RpcError::TxnOutputCommitmentsExistedRecently(
                    ElementsVecData {
                        elements: vec![leaf],
                    },
                ))?;
            }
        }
    }

    for leaf in utxo_proof.public_inputs.input_commitments {
        if leaf >= Element::MODULUS {
            Err(RpcError::InvalidElementSize(ElementData { element: leaf }))?;
        }

        if leaf != Element::ZERO && !tree.contains_element(&leaf) {
            Err(RpcError::TxnInputCommitmentsNotInTree(ElementsVecData {
                elements: vec![leaf],
            }))?;
        }
    }

    let mint_hash = match utxo_proof.kind_messages() {
        zk_primitives::UtxoKindMessages::Mint(utxo_kind_mint_messages) => {
            Some(utxo_kind_mint_messages.mint_hash)
        }
        zk_primitives::UtxoKindMessages::Burn(_) => None,
        zk_primitives::UtxoKindMessages::None => None,
    };

    if let Some(mint_hash) = mint_hash {
        let mint_hash_in_db = block_store.get_mint_hash(mint_hash)?;
        if mint_hash_in_db.is_some() {
            Err(RpcError::MintHashAlreadyExists(ElementData {
                element: mint_hash,
            }))?;
        }
    }

    Ok(())
}
