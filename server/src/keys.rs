use anyhow::{anyhow, Result};
use k256::{elliptic_curve::sec1::ToEncodedPoint, SecretKey};
use sha3::{Digest, Sha3_256};

pub struct KeyMaterial {
    pub private_key: [u8; 32],
    pub public_key: Vec<u8>,
}

/// Derive a deterministic secp256k1 key pair from an arbitrary name.
pub fn derive_key_material(name: &str) -> Result<KeyMaterial> {
    if name.trim().is_empty() {
        return Err(anyhow!("name must not be empty"));
    }

    let mut counter: u32 = 0;
    loop {
        let mut hasher = Sha3_256::new();
        hasher.update(name.as_bytes());
        hasher.update(counter.to_be_bytes());
        let digest: [u8; 32] = hasher.finalize().into();

        match SecretKey::from_slice(&digest) {
            Ok(secret) => {
                let public_key = secret.public_key();
                let encoded = public_key.to_encoded_point(false);

                return Ok(KeyMaterial {
                    private_key: digest,
                    public_key: encoded.as_bytes().to_vec(),
                });
            }
            Err(_) => {
                counter = counter
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("failed to derive key material for provided name"))?;
            }
        }
    }
}
