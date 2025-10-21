pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to dial peer: {0}")]
    DialPeer(#[from] libp2p::swarm::DialError),

    #[error("Tansport error: {0}")]
    Transport(#[from] libp2p::TransportError<std::io::Error>),

    #[error("Channel error")]
    ChannelError(String),
}
