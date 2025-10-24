use anyhow::{anyhow, Context, Result};
use borsh::{BorshDeserialize, BorshSerialize};
use client_sdk::transaction_builder::TxExecutorHandler;
use hex::encode as hex_encode;
use hyli_utxo_state::{
    state::{HyliUtxoState, HyliUtxoStateAction},
    zk::BorshableH256,
    HyliUtxoZkVmState,
};
use sdk::{
    utils::{as_hyli_output, parse_raw_calldata},
    Blob, Calldata, Contract, ContractName, HyliOutput, RunResult, StateCommitment,
};
use tracing::info;

#[derive(Debug, Default, BorshSerialize, BorshDeserialize)]
pub struct HyliUtxoStateExecutor {
    state: HyliUtxoState,
}

impl Clone for HyliUtxoStateExecutor {
    fn clone(&self) -> Self {
        let encoded = borsh::to_vec(&self.state).expect("HyliUtxoState should serialize for clone");
        let state =
            borsh::from_slice(&encoded).expect("HyliUtxoState should deserialize for clone");
        Self { state }
    }
}

impl HyliUtxoStateExecutor {
    pub fn zkvm_witness(
        &self,
        note_keys: &[BorshableH256],
        nullified_keys: &[BorshableH256],
    ) -> Result<HyliUtxoZkVmState> {
        self.state
            .to_zkvm_state(note_keys, nullified_keys)
            .map_err(|e| anyhow!(e))
    }

    fn apply_commitments(
        &mut self,
        nullified: &[BorshableH256],
        created: &[BorshableH256],
    ) -> Result<()> {
        if !nullified.is_empty() {
            self.state
                .record_nullified(nullified)
                .map_err(|e| anyhow!(e))?;
        }
        if !created.is_empty() {
            self.state.record_created(created).map_err(|e| anyhow!(e))?;
        }
        Ok(())
    }

    fn update_from_blob(
        &mut self,
        calldata: &Calldata,
    ) -> Result<(Vec<BorshableH256>, Vec<BorshableH256>)> {
        let (_, state_blob) = calldata
            .blobs
            .iter()
            .find(|(index, _)| *index == calldata.index)
            .ok_or_else(|| anyhow!("state blob not found in calldata"))?;

        let action: HyliUtxoStateAction =
            BorshDeserialize::try_from_slice(&state_blob.data.0).map_err(|e| anyhow!(e))?;
        let (created, nullified) = Self::split_action(&action);
        info!(
            created_len = created.len(),
            nullified_len = nullified.len(),
            blob_index = %calldata.index.0,
            "applying hyli_utxo_state action"
        );
        self.apply_commitments(&nullified, &created)?;
        Ok((created, nullified))
    }

    fn split_action(action: &HyliUtxoStateAction) -> (Vec<BorshableH256>, Vec<BorshableH256>) {
        let created = action
            .iter()
            .take(2)
            .copied()
            .filter(|commitment| {
                let bytes: [u8; 32] = commitment.0.into();
                bytes != [0u8; 32]
            })
            .collect();

        let nullified = action
            .iter()
            .skip(2)
            .copied()
            .filter(|commitment| {
                let bytes: [u8; 32] = commitment.0.into();
                bytes != [0u8; 32]
            })
            .collect();

        (created, nullified)
    }
}

impl TxExecutorHandler for HyliUtxoStateExecutor {
    type Contract = Self;

    fn construct_state(
        _contract_name: &ContractName,
        _contract: &Contract,
        metadata: &Option<Vec<u8>>,
    ) -> Result<Self> {
        let state = match metadata {
            Some(bytes) if !bytes.is_empty() => {
                borsh::from_slice(bytes).context("decoding HyliUtxoState")?
            }
            _ => HyliUtxoState::default(),
        };
        Ok(HyliUtxoStateExecutor { state })
    }

    fn build_commitment_metadata(&self, blob: &Blob) -> Result<Vec<u8>> {
        let action: HyliUtxoStateAction =
            BorshDeserialize::try_from_slice(&blob.data.0).map_err(|e| anyhow!(e))?;

        let (created, nullified) = Self::split_action(&action);

        info!(
            created_len = created.len(),
            nullified_len = nullified.len(),
            blob_contract = %blob.contract_name.0,
            "built hyli_utxo_state commitment metadata"
        );

        let witness = self.zkvm_witness(&created, &nullified)?;
        borsh::to_vec(&witness).context("serializing HyliUtxoZkVmState")
    }

    fn handle(&mut self, calldata: &Calldata) -> Result<HyliOutput> {
        let initial_commitment = self.get_state_commitment();

        let (_, execution_ctx) = parse_raw_calldata::<HyliUtxoStateAction>(calldata)
            .map_err(|e| anyhow!("parsing calldata: {e}"))?;

        let (created, nullified) = self.update_from_blob(calldata)?;

        let next_commitment = self.get_state_commitment();
        let initial_hex = hex_encode(&initial_commitment.0);
        let next_hex = hex_encode(&next_commitment.0);
        info!(
            created_len = created.len(),
            nullified_len = nullified.len(),
            initial_commitment = %initial_hex,
            next_commitment = %next_hex,
            "executed hyli_utxo_state action"
        );

        let mut result: RunResult = Ok((Vec::new(), execution_ctx, Vec::new()));

        Ok(as_hyli_output(
            initial_commitment,
            next_commitment,
            calldata,
            &mut result,
        ))
    }

    fn get_state_commitment(&self) -> StateCommitment {
        self.state.commitment()
    }
}
