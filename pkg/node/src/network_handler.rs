use crate::network::{NetworkEvent, SnapshotAccept, SnapshotOffer, SnapshotRequest};
use crate::node::NodeShared;
use eyre::Context;
use libp2p::PeerId;
use p2p2::Network;
use std::sync::Arc;
use tokio::task::JoinHandle;

pub fn network_handler(
    network: Arc<Network<NetworkEvent>>,
    node: Arc<NodeShared>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let Some((network_peer_id, event)) = network.next().await else {
                continue;
            };
            tracing::debug!(network_peer_id = ?network_peer_id, event = ?event, "network event");

            if let Err(e) = handle_event(&node, network_peer_id, event).await {
                tracing::error!(error = ?e, "network error");
            }
        }
    })
}

async fn handle_event(
    node: &NodeShared,
    peer: PeerId,
    event: NetworkEvent,
) -> color_eyre::Result<()> {
    match event {
        NetworkEvent::Approval(approval) => node
            .receive_accept(&approval)
            .await
            .context("Accept failed")?,

        NetworkEvent::Block(block) => {
            node.receive_proposal(block)
                .context("Failed to process block")?;
            node.ticker.tick();
        }

        NetworkEvent::Transaction(txn) => node
            .receive_transaction(txn)
            .await
            .context("Transaction failed")?,

        NetworkEvent::SnapshotRequest(SnapshotRequest {
            snapshot_id,
            from_height,
            to_height,
            kind,
        }) => node
            .receive_snapshot_request(peer, snapshot_id, from_height, to_height, kind)
            .await
            .context("Snapshot request failed")?,

        NetworkEvent::SnapshotOffer(SnapshotOffer { snapshot_id }) => node
            .receive_snapshot_offer(peer, snapshot_id)
            .context("Snapshot offer failed")?,

        NetworkEvent::SnapshotChunk(sc) => node
            .receive_snapshot_chunk(peer, sc)
            .context("Snapshot chunk failed")?,

        NetworkEvent::SnapshotAccept(SnapshotAccept {
            snapshot_id,
            from_height,
            to_height,
            kind,
        }) => node
            .receive_snapshot_accept(peer, snapshot_id, from_height, to_height, kind)
            .await
            .context("Snapshot accept failed")?,
    }

    Ok(())
}
