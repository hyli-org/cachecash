use borsh::{BorshDeserialize, BorshSerialize};
use libp2p::{Multiaddr, PeerId};
use tokio::sync::oneshot;

/// A command that can be sent to a running P2P node
#[derive(Debug)]
pub enum Command<NetworkEvent>
where
    NetworkEvent: Clone + Send + BorshSerialize + BorshDeserialize + 'static,
{
    /// Dial another node running at the provided address
    Dial(
        Multiaddr,
        oneshot::Sender<Result<(), libp2p::swarm::DialError>>,
    ),

    /// Send a message to another peer, Sender will respond when response
    /// received
    Send(PeerId, NetworkEvent, oneshot::Sender<()>),
}
