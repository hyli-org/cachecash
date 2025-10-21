use contracts::{Address, ConfirmationType, RollupContract, USDCContract};
use element::Element;
use eth_util::Eth;
use eyre::{Context, ContextCompat};
use primitives::{
    block_height::BlockHeight,
    pagination::{CursorChoice, CursorChoiceAfter, OpaqueCursor, OpaqueCursorChoice},
};
use reqwest::StatusCode;
use std::time::Duration;
use zk_primitives::{UtxoKindMessages, UtxoProof};

pub struct BurnSubstitutor {
    rollup_contract: RollupContract,
    usdc_contract: USDCContract,
    node_rpc_url: String,
    eth_txn_confirm_wait_interval: Duration,
    cursor: Option<OpaqueCursorChoice<ListTxnsPosition>>,
}

impl BurnSubstitutor {
    pub fn new(
        rollup_contract: RollupContract,
        usdc_contract: USDCContract,
        node_rpc_url: String,
        eth_txn_confirm_wait_interval: Duration,
    ) -> Self {
        BurnSubstitutor {
            rollup_contract,
            usdc_contract,
            node_rpc_url,
            eth_txn_confirm_wait_interval,
            cursor: None,
        }
    }

    pub async fn tick(&mut self) -> Result<Vec<Element>, eyre::Error> {
        if self.cursor.is_none() {
            let last_rollup = self.fetch_last_rollup_block().await?;

            self.cursor = Some(
                CursorChoice::After(CursorChoiceAfter::After(ListTxnsPosition {
                    block: last_rollup,
                    txn: u64::MAX,
                }))
                .opaque(),
            );
        }

        let (txns, cursor) = Self::fetch_transactions(
            &reqwest::Client::new(),
            &self.node_rpc_url,
            None,
            self.cursor.as_ref(),
            false,
        )
        .await
        .context("Failed to fetch transactions")?;

        let mut substituted_burns = Vec::new();
        for txn in &txns {
            if let UtxoKindMessages::Burn(burn_msgs) = txn.proof.kind_messages() {
                let hash = burn_msgs.burn_hash;
                let burn_address =
                    Address::from_slice(&burn_msgs.burn_address.to_be_bytes()[12..32]);
                let amount = burn_msgs.value;
                let note_kind = burn_msgs.note_kind;

                if self
                    .rollup_contract
                    .was_burn_substituted(
                        &burn_address,
                        &note_kind,
                        &hash,
                        &amount,
                        txn.block_height.0,
                    )
                    .await?
                {
                    continue;
                }

                // Calculate the burn value as an EVM U256
                let burn_value = burn_msgs.value.to_eth_u256();

                // Check USDC balance and optionally skip if burn exceeds available balance
                let usdc_balance = self
                    .usdc_contract
                    .balance(self.rollup_contract.signer_address)
                    .await
                    .context("Failed to fetch USDC balance for burn substitution")?;

                if burn_value > usdc_balance {
                    tracing::info!(
                        ?txn.proof.public_inputs,
                        %burn_value,
                        %usdc_balance,
                        "Skipping burn: value exceeds substitutor balance"
                    );
                    continue;
                }

                let txn = self
                    .rollup_contract
                    .substitute_burn(
                        &burn_address,
                        &note_kind,
                        &hash,
                        &amount,
                        txn.block_height.0,
                    )
                    .await
                    .context("Failed to substitute burn")?;

                self.rollup_contract
                    .client
                    .wait_for_confirm(
                        txn,
                        self.eth_txn_confirm_wait_interval,
                        ConfirmationType::Latest,
                    )
                    .await
                    .context("Failed to wait for burn substitution")?;

                substituted_burns.push(hash);
            }
        }

        if !txns.is_empty() {
            self.cursor = cursor
                .after
                .map(|after| CursorChoice::After(after.0).opaque());
        }

        Ok(substituted_burns)
    }

    async fn fetch_last_rollup_block(&mut self) -> Result<BlockHeight, contracts::Error> {
        self.rollup_contract.block_height().await.map(BlockHeight)
    }

    async fn fetch_transactions(
        client: &reqwest::Client,
        network_base_url: &str,
        limit: Option<usize>,
        cursor: Option<&OpaqueCursorChoice<ListTxnsPosition>>,
        poll: bool,
    ) -> Result<(Vec<Transaction>, OpaqueCursor<ListTxnsPosition>), eyre::Error> {
        let req = client
            .get(format!("{network_base_url}/v0/transactions"))
            .query(&[
                ("limit", limit.map(|l| l.to_string())),
                ("order", Some("OldestToNewest".to_owned())),
                ("cursor", cursor.map(|c| c.serialize()).transpose()?),
                ("poll", Some(poll.to_string())),
            ]);

        let resp = req.send().await?;

        match resp.status() {
            StatusCode::OK => {}
            e => return Err(eyre::eyre!("Unexpected status code: {e}")),
        }

        let mut resp = resp.json::<serde_json::Value>().await?;

        let txns = serde_json::from_value::<Vec<Transaction>>(
            resp.get_mut("txns").context("Missing txns field")?.take(),
        )?;

        let cursor = resp
            .get_mut("cursor")
            .context("Missing pagination field")?
            .take();

        let cursor = serde_json::from_value(cursor).context("Failed to parse cursor")?;

        Ok((txns, cursor))
    }
}

#[derive(Debug, serde::Deserialize)]
struct Transaction {
    pub proof: UtxoProof,
    pub block_height: BlockHeight,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct ListTxnsPosition {
    block: BlockHeight,
    txn: u64,
}
