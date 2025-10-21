use element::{Base, Element};
use primitives::serde::{deserialize_base64, serialize_base64};
use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-rs")]
use ts_rs::TS;

use crate::ToBytes;

/// Migration input for proving ownership during address migration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Migrate {
    /// The private key of the owner
    pub owner_pk: Element,
    /// The old address (public input)
    pub old_address: Element,
    /// The new address (public input)
    pub new_address: Element,
}

impl Migrate {
    /// Create a new migration input
    #[must_use]
    pub fn new(owner_pk: Element, old_address: Element, new_address: Element) -> Self {
        Self {
            owner_pk,
            old_address,
            new_address,
        }
    }
}

/// Migration proof bytes wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
pub struct MigrateProofBytes(
    #[serde(
        serialize_with = "serialize_base64",
        deserialize_with = "deserialize_base64"
    )]
    #[cfg_attr(feature = "ts-rs", ts(as = "String"))]
    pub Vec<u8>,
);

/// Migration public inputs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
pub struct MigratePublicInput {
    /// The old address being migrated from
    #[cfg_attr(feature = "ts-rs", ts(as = "String"))]
    pub old_address: Element,
    /// The new address being migrated to
    #[cfg_attr(feature = "ts-rs", ts(as = "String"))]
    pub new_address: Element,
}

impl MigratePublicInput {
    /// Convert the public inputs to bytes
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        [
            self.old_address.to_be_bytes(),
            self.new_address.to_be_bytes(),
        ]
        .concat()
    }

    /// Convert the public inputs to field elements
    #[must_use]
    pub fn to_fields(&self) -> Vec<Base> {
        vec![self.old_address.to_base(), self.new_address.to_base()]
    }
}

/// Migration proof output
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(TS))]
#[cfg_attr(feature = "ts-rs", ts(export))]
pub struct MigrateProof {
    /// The proof bytes (without public inputs)
    pub proof: MigrateProofBytes,
    /// The public inputs
    pub public_inputs: MigratePublicInput,
}

impl ToBytes for MigrateProof {
    /// Convert the migrate proof to bytes
    fn to_bytes(&self) -> Vec<u8> {
        let pi = self.public_inputs.to_bytes();
        let proof = self.proof.0.clone();
        [pi.as_slice(), proof.as_slice()].concat()
    }
}

#[cfg(all(test, feature = "ts-rs"))]
mod test {
    use super::*;
    use ts_rs::TS;

    #[test]
    fn export_bindings_migrate_proof_bytes() {
        unsafe {
            std::env::set_var(
                "TS_RS_EXPORT_DIR",
                "../../app/packages/payy/src/ts-rs-bindings/",
            );
        }
        MigrateProofBytes::export().expect("failed to export MigrateProofBytes");
    }

    #[test]
    fn export_bindings_migrate_public_input() {
        unsafe {
            std::env::set_var(
                "TS_RS_EXPORT_DIR",
                "../../app/packages/payy/src/ts-rs-bindings/",
            );
        }
        MigratePublicInput::export().expect("failed to export MigratePublicInput");
    }

    #[test]
    fn export_bindings_migrate_proof() {
        unsafe {
            std::env::set_var(
                "TS_RS_EXPORT_DIR",
                "../../app/packages/payy/src/ts-rs-bindings/",
            );
        }
        MigrateProof::export().expect("failed to export MigrateProof");
    }
}
