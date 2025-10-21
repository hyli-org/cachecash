use primitives::peer::PeerIdSigner;

fn main() {
    color_eyre::install().unwrap();

    let peer_signer = PeerIdSigner::default();
    println!("Secret key:");
    println!("  0x{}", peer_signer.to_hex());
    println!();
    println!("Peer ID:");
    println!("  0x{}", peer_signer.address().to_hex());
    println!();
}
