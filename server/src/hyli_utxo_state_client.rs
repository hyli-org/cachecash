use anyhow::{anyhow, Context, Result};
use borsh::{BorshDeserialize, BorshSerialize};
use client_sdk::transaction_builder::TxExecutorHandler;
use hyli_utxo_state::{
    state::{hyli_utxo_blob, parse_hyli_utxo_blob, HyliUtxoState, HyliUtxoStateAction},
    zk::BorshableH256,
    HyliUtxoZkVmState,
};
use sdk::{
    utils::{as_hyli_output, parse_raw_calldata},
    Blob, Calldata, Contract, ContractName, HyliOutput, RunResult, StateCommitment,
};

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

    fn update_from_blob(&mut self, calldata: &Calldata) -> Result<()> {
        let blob_bytes = hyli_utxo_blob(calldata).map_err(|e| anyhow!(e))?;
        let (nullified, created) = parse_hyli_utxo_blob(blob_bytes).map_err(|e| anyhow!(e))?;
        self.apply_commitments(&nullified, &created)
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
        let (nullified, created) = parse_hyli_utxo_blob(&blob.data.0).map_err(|e| anyhow!(e))?;
        let witness = self.zkvm_witness(&created, &nullified)?;
        borsh::to_vec(&witness).context("serializing HyliUtxoZkVmState")
    }

    fn handle(&mut self, calldata: &Calldata) -> Result<HyliOutput> {
        let initial_commitment = self.get_state_commitment();

        let (_, execution_ctx) = parse_raw_calldata::<HyliUtxoStateAction>(calldata)
            .map_err(|e| anyhow!("parsing calldata: {e}"))?;

        self.update_from_blob(calldata)?;

        let next_commitment = self.get_state_commitment();
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
