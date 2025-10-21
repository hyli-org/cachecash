use zk_primitives::ToBytes;

use crate::Result;

pub trait Prove {
    type Result<Proof>;
    type Proof: ToBytes + Verify;

    fn prove(&self) -> Result<Self::Proof>;
}

pub trait Verify: ToBytes {
    #[must_use = "verification result must be explicitly handled"]
    fn verify(&self) -> Result<()>;
}
