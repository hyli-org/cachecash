extern crate rand;

use libp2p::identity;
use rand::RngCore;

pub(crate) fn generate_p2p_key() -> (identity::Keypair, [u8; 32]) {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    #[allow(clippy::unwrap_used)]
    let keypair = identity::Keypair::ed25519_from_bytes(bytes).unwrap();
    (keypair, bytes)
}
