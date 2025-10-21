mod backend;
mod circuits;
mod execute;
mod prove;
mod traits;
mod util;
pub mod verify;

pub use circuits::AGG_UTXO_VERIFICATION_KEY_HASH;
pub use traits::{Prove, Verify};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn prove_agg_agg() {
//         use noirc_abi::input_parser::Format;
//         use noirc_abi::{input_parser::InputValue, Abi, InputMap};
//         use std::collections::BTreeMap;
//         use std::path::Path;

//         fn read_inputs_from_file<P: AsRef<Path>>(
//             path: P,
//             file_name: &str,
//             format: Format,
//             abi: &Abi,
//         ) -> std::result::Result<(InputMap, Option<InputValue>), Box<dyn std::error::Error>>
//         {
//             if abi.is_empty() {
//                 return Ok((BTreeMap::new(), None));
//             }

//             let file_path = path.as_ref().join(file_name).with_extension(format.ext());
//             if !file_path.exists() {
//                 if abi.parameters.is_empty() {
//                     // Reading a return value from the `Prover.toml` is optional,
//                     // so if the ABI has no parameters we can skip reading the file if it doesn't exist.
//                     return Ok((BTreeMap::new(), None));
//                 } else {
//                     panic!("{:?}", (file_name.to_owned(), file_path,));
//                 }
//             }

//             let input_string = std::fs::read_to_string(file_path).unwrap();
//             let mut input_map = format.parse(&input_string, abi)?;
//             let return_value = input_map.remove(noirc_abi::MAIN_RETURN_NAME);

//             Ok((input_map, return_value))
//         }

//         let program = execute::parse_program(utxo::PROGRAM).unwrap();

//         let owner_pk = Fr::try_from_str("101").unwrap();
//         let address = Barretenberg::poseidon_hash(&[owner_pk, Fr::zero()]);

//         let input_notes = [
//             utxo::Note {
//                 address,
//                 kind: Fr::try_from_str("1").unwrap(),
//                 psi: Fr::try_from_str("1").unwrap(),
//                 value: Fr::try_from_str("10").unwrap(),
//             },
//             utxo::Note {
//                 address,
//                 kind: Fr::try_from_str("1").unwrap(),
//                 psi: Fr::try_from_str("2").unwrap(),
//                 value: Fr::try_from_str("5").unwrap(),
//             },
//         ];
//         let output_notes = [
//             utxo::Note {
//                 address,
//                 kind: Fr::try_from_str("1").unwrap(),
//                 psi: Fr::try_from_str("3").unwrap(),
//                 value: Fr::try_from_str("1").unwrap(),
//             },
//             utxo::Note {
//                 address,
//                 kind: Fr::try_from_str("1").unwrap(),
//                 psi: Fr::try_from_str("4").unwrap(),
//                 value: Fr::try_from_str("14").unwrap(),
//             },
//         ];

//         let input_commitments = input_notes
//             .clone()
//             .into_iter()
//             .map(|n| Barretenberg::poseidon_hash(&[n.kind, n.value, n.address, n.psi]));
//         let output_commitments = output_notes
//             .clone()
//             .into_iter()
//             .map(|n| Barretenberg::poseidon_hash(&[n.kind, n.value, n.address, n.psi]));
//         let commitments = input_commitments
//             .clone()
//             .chain(output_commitments.clone())
//             .collect::<Vec<_>>();

//         let my_input = utxo::Input {
//             kind: Fr::one(),
//             message: Fr::zero(),
//             owner_pk,
//             commitments: commitments.clone().try_into().unwrap(),
//             input_notes,
//             output_notes,
//         };

//         // let (input_map, _) = read_inputs_from_file(
//         //     "/Users/hwq/Documents/polybase/noir-circuits/utxo",
//         //     "Prover",
//         //     Format::Toml,
//         //     &program.abi,
//         // )
//         // .unwrap();
//         // assert_eq!(InputMap::from(my_input.clone()), input_map);

//         let results = execute::execute_program_and_decode(
//             program.into(),
//             &InputMap::from(my_input.clone()),
//             false,
//         )
//         .unwrap();
//         let witness_gz = TryInto::<Vec<u8>>::try_into(results.witness_stack).unwrap();

//         let bytecode = utxo::BYTECODE;

//         let bb = Barretenberg::new(bytecode, utxo::PROGRAM);
//         // let proof = bb.prove(&witness_gz, true).unwrap();
//         let proof = bb.utxo_prove(my_input.clone()).unwrap();
//         let proof_as_fields = bb.proof_as_fields(utxo::KEY, &proof).unwrap();
//         let proof_as_fields_without_public_inputs =
//             bb.utxo_proof_without_public_inputs(&proof_as_fields);

//         assert!(bb.utxo_verify(&proof).unwrap());

//         let utxo_vk = bb.generate_vk().unwrap();
//         let (utxo_vk_hash, utxo_vk_fields) = bb.vk_as_fields(&utxo_vk).unwrap();

//         let mut tree = smirk::Tree::<129, ()>::new();

//         let mut merkle_paths = Vec::new();
//         for commitment in input_commitments.clone() {
//             tree.insert(smirk::Element::from_base(commitment.into()), ())
//                 .unwrap();

//             let path = tree.path_for(smirk::Element::from_base(commitment.into()));

//             let path: [Fr; 128] = path
//                 .siblings_deepest_first()
//                 .iter()
//                 .cloned()
//                 .take(128)
//                 .map(|e| e.to_base().0)
//                 .collect::<Vec<_>>()
//                 .try_into()
//                 .unwrap();

//             merkle_paths.push(path);
//         }

//         let old_root = tree.root_hash();

//         for commitment in output_commitments.clone() {
//             tree.insert(smirk::Element::from_base(commitment.into()), ())
//                 .unwrap();

//             let path = tree.path_for(smirk::Element::from_base(commitment.into()));

//             let path: [Fr; 128] = path
//                 .siblings_deepest_first()
//                 .iter()
//                 .cloned()
//                 .take(128)
//                 .map(|e| e.to_base().0)
//                 .collect::<Vec<_>>()
//                 .try_into()
//                 .unwrap();

//             merkle_paths.push(path);
//         }

//         let merkle_paths: [_; 4] = merkle_paths.try_into().unwrap();

//         let agg_utxo_input = agg_utxo::Input {
//             verification_key: utxo_vk_fields.try_into().unwrap(),
//             proofs: std::iter::repeat_n(
//                 agg_utxo::UtxoProof {
//                     proof: proof_as_fields_without_public_inputs.try_into().unwrap(),
//                     merkle_paths,
//                     input_commitments: input_commitments
//                         .map(|f| {
//                             let mut be_bytes = smirk::Element::from_base(f.into()).to_be_bytes();
//                             for i in (128 / 8)..(256 / 8) {
//                                 be_bytes[31 - i] = 0;
//                             }

//                             smirk::Element::from_be_bytes(be_bytes).to_base().0
//                         })
//                         .collect::<Vec<_>>()
//                         .try_into()
//                         .unwrap(),
//                     output_commitments: output_commitments
//                         .map(|f| {
//                             let mut be_bytes = smirk::Element::from_base(f.into()).to_be_bytes();
//                             for i in (128 / 8)..(256 / 8) {
//                                 be_bytes[31 - i] = 0;
//                             }

//                             smirk::Element::from_be_bytes(be_bytes).to_base().0
//                         })
//                         .collect::<Vec<_>>()
//                         .try_into()
//                         .unwrap(),
//                 },
//                 3,
//             )
//             .collect::<Vec<_>>()
//             .try_into()
//             .unwrap(),
//             kinds: std::iter::repeat_n(my_input.kind, 3)
//                 .collect::<Vec<_>>()
//                 .try_into()
//                 .unwrap(),
//             old_root: old_root.to_base(),
//             new_root: tree.root_hash().to_base(),
//             key_hash: utxo_vk_hash,
//         };

//         let bb = Barretenberg::new_agg_utxo();
//         let proof = bb.agg_utxo_prove(agg_utxo_input.clone()).unwrap();
//         let proof_as_fields = bb.proof_as_fields(agg_utxo::KEY, &proof).unwrap();
//         let proof_as_fields_without_public_inputs =
//             bb.agg_utxo_proof_without_public_inputs(&proof_as_fields);

//         let utxo_agg_vk = bb.generate_vk().unwrap();
//         let (utxo_agg_vk_hash, utxo_vk_fields) = bb.vk_as_fields(&utxo_agg_vk).unwrap();

//         let bb = Barretenberg::new_agg_agg();

//         let agg_agg_input = agg_agg::Input {
//             verification_key: utxo_vk_fields.try_into().unwrap(),
//             proofs: std::iter::repeat_n(
//                 agg_agg::AggUtxoProof {
//                     proof: proof_as_fields_without_public_inputs.try_into().unwrap(),
//                     old_root: agg_utxo_input.old_root,
//                     new_root: agg_utxo_input.new_root,
//                 },
//                 2,
//             )
//             .collect::<Vec<_>>()
//             .try_into()
//             .unwrap(),
//             kinds: std::iter::repeat_n(agg_utxo_input.kinds.iter(), 2)
//                 .flatten()
//                 .copied()
//                 .collect::<Vec<_>>()
//                 .try_into()
//                 .unwrap(),
//             old_root: agg_utxo_input.old_root,
//             new_root: agg_utxo_input.new_root,
//             key_hash: agg_utxo_input.key_hash,
//         };

//         bb.agg_agg_prove(agg_agg_input).unwrap();
//     }
// }
