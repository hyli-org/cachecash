use crate::{
    Block, Error, NodeShared, Result, mempool::AddError, network::NetworkEvent, utxo::validate_txn,
};
use ethereum_types::U64;
use node_interface::{ElementData, ElementsVecData, MintInContractIsDifferent, RpcError};
use std::{sync::Arc, time::Duration};
use tracing::{error, info, instrument};
use zk_primitives::{UtxoKindMessages, UtxoProof};

impl NodeShared {
    pub async fn submit_transaction_and_wait(&self, utxo: UtxoProof) -> Result<Arc<Block>> {
        let mut started_waiting_at_eth_block = None;
        loop {
            match self.validate_transaction(&utxo).await {
                Ok(_) => break,
                Err(err) => match &err {
                    Error::Rpc(RpcError::MintIsNotInTheContract(..)) => {
                        let current_eth_block = self
                            .rollup_contract
                            .client
                            .client()
                            .eth()
                            .block_number()
                            .await
                            .map_err(Error::FailedToGetEthBlockNumber)?
                            .as_u64();
                        let started_waiting_at_eth_block =
                            *started_waiting_at_eth_block.get_or_insert(current_eth_block);

                        let waited_too_long_for_confirmation = current_eth_block
                            - started_waiting_at_eth_block
                            > self.config.safe_eth_height_offset;

                        // TODO: we could wait a little extra time and accept mints/burns
                        // that are not even valid at `latest` height yet,
                        // because they are still in eth mempool
                        if self.config.safe_eth_height_offset == 0
                            || waited_too_long_for_confirmation
                        {
                            return Err(err);
                        }
                    }
                    _ => return Err(err),
                },
            }

            tokio::time::sleep(Duration::from_secs(6)).await;
        }

        self.send_all(NetworkEvent::Transaction(utxo.clone())).await;

        let mut changes = Vec::new();
        for commitment in utxo
            .public_inputs
            .input_commitments
            .into_iter()
            .chain(utxo.public_inputs.output_commitments)
        {
            if commitment.is_zero() {
                continue;
            }
            if !changes.contains(&commitment) {
                changes.push(commitment);
            }
        }

        let receiver = match self.mempool.add_with_listener(utxo.hash(), utxo, changes) {
            Ok(receiver) => receiver,
            Err(AddError::Conflict(conflict)) => {
                return Err(RpcError::TxnCommitmentAlreadyPending(ElementsVecData {
                    elements: vec![conflict],
                })
                .into());
            }
            Err(AddError::DuplicateKey) => {
                return Err(RpcError::TxnCommitmentAlreadyPending(ElementsVecData {
                    elements: vec![],
                })
                .into());
            }
        };

        receiver.await.expect("recv error")
    }

    pub(super) async fn validate_transaction(&self, utxo: &UtxoProof) -> Result<()> {
        if let UtxoKindMessages::Mint(mint_msgs) = utxo.kind_messages() {
            let eth_block = self
                .rollup_contract
                .client
                .client()
                .eth()
                .block_number()
                .await
                .map_err(Error::FailedToGetEthBlockNumber)?;

            let safe_eth_height =
                match eth_block.overflowing_sub(U64::from(self.config.safe_eth_height_offset)) {
                    (safe_eth_height, false) => safe_eth_height,
                    // This can happen if we are running with a local hardhat node
                    (_, true) => U64::from(0),
                };
            let rollup_contract_at_safe_height = self
                .rollup_contract
                .clone()
                .at_height(Some(safe_eth_height.as_u64()));

            let Some(get_mint_res) = rollup_contract_at_safe_height
                .get_mint(&mint_msgs.mint_hash)
                .await?
            else {
                return Err(RpcError::MintIsNotInTheContract(ElementData {
                    element: mint_msgs.mint_hash,
                }))?;
            };

            // Check if mint is already spent
            if get_mint_res.spent {
                return Err(RpcError::MintIsAlreadySpent(ElementsVecData {
                    elements: utxo.public_inputs.output_commitments.to_vec(),
                }))?;
            }

            // Check mint amout/kind matches the submitted utxo proof
            if get_mint_res.amount != mint_msgs.value
                || get_mint_res.note_kind != mint_msgs.note_kind
            {
                return Err(RpcError::MintInContractIsDifferent(Box::new(
                    MintInContractIsDifferent {
                        contract_value: get_mint_res.amount,
                        contract_note_kind: get_mint_res.note_kind,
                        proof_value: mint_msgs.value,
                        proof_note_kind: mint_msgs.note_kind,
                    },
                )))?;
            }
        }

        validate_txn(
            self.config.mode,
            utxo,
            self.height(),
            &self.block_store,
            &self.notes_tree.read(),
        )
    }

    #[instrument(skip(self, txn))]
    pub async fn receive_transaction(&self, txn: UtxoProof) -> Result<()> {
        info!("Received transaction");

        if let Err(err) = self.validate_transaction(&txn).await {
            error!(
                ?err,
                "Failed to validate transaction received from another node"
            );
            return Ok(());
        }

        let mut changes = Vec::new();
        for commitment in txn
            .public_inputs
            .input_commitments
            .into_iter()
            .chain(txn.public_inputs.output_commitments)
        {
            if commitment.is_zero() {
                continue;
            }
            if !changes.contains(&commitment) {
                changes.push(commitment);
            }
        }

        match self.mempool.add(txn.hash(), txn, changes) {
            Ok(()) => {}
            Err(AddError::Conflict(conflict)) => {
                return Err(RpcError::TxnCommitmentAlreadyPending(ElementsVecData {
                    elements: vec![conflict],
                })
                .into());
            }
            Err(AddError::DuplicateKey) => {
                return Ok(());
            }
        }

        Ok(())
    }
}
