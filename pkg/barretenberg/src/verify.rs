use crate::{Result, backend::Backend};
use element::Base;

pub fn verify<B: Backend>(key: &[u8], proof: &[u8], oracle_hash_keccak: bool) -> Result<()> {
    B::verify(proof, key, oracle_hash_keccak)
}

// pub fn verify(bb_path: &PathBuf, key: &[u8], proof: &[u8]) -> Result<bool> {
//     let mut key_file = NamedTempFile::new()?;
//     key_file.write_all(key)?;
//     key_file.flush()?;

//     let header: u32 = proof.len() as u32;
//     let header_bytes = header.to_be_bytes();
//     println!("{:?}", header);
//     println!("{:?}", header_bytes);
//     let mut proof_with_header = Vec::with_capacity(header_bytes.len() + proof.len());
//     proof_with_header.extend_from_slice(&header_bytes);
//     proof_with_header.extend_from_slice(proof);

//     let mut proof_file = NamedTempFile::new()?;
//     proof_file.write_all(proof_with_header.as_slice())?;
//     proof_file.flush()?;

//     let mut cmd = Command::new(bb_path);
//     cmd.arg("verify")
//         .arg("--scheme")
//         .arg("ultra_honk")
//         .arg("-k")
//         .arg(key_file.path())
//         .arg("-p")
//         .arg(proof_file.path());

//     let output = cmd.output()?;

//     println!("output: {:?}", output);
//     if !output.status.success() {
//         let stderr = String::from_utf8(output.stderr)?;
//         return Err(stderr.into());
//     }

//     Ok(true)
// }

#[derive(Debug, Clone)]
pub struct VerificationKeyHash(pub Base);

#[derive(Debug, Clone)]
pub struct VerificationKey(pub Vec<Base>);
