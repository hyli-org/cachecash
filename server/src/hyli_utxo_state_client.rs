use anyhow::{anyhow, Context, Result};
use borsh::{BorshDeserialize, BorshSerialize};
use client_sdk::transaction_builder::TxExecutorHandler;
use hyli_utxo_state::{
    state::{hyli_utxo_blob, parse_hyli_utxo_blob, HyliUtxoState},
    zk::BorshableH256,
    HyliUtxoZkVmState,
};
use sdk::{
    utils::{as_hyli_output, parse_raw_calldata},
    Blob, Calldata, Contract, ContractName, HyliOutput, RunResult, StateCommitment, ZkContract,
};

#[derive(Debug, Default)]
pub struct HyliUtxoStateExecutor {
    state: HyliUtxoState,
}

impl Clone for HyliUtxoStateExecutor {
    fn clone(&self) -> Self {
        Self::from_zkvm_state(
            self.zkvm_state()
                .expect("HyliUtxoState can convert to zkvm state"),
        )
        .expect("HyliUtxoStateExecutor clone conversion must succeed")
    }
}

impl BorshSerialize for HyliUtxoStateExecutor {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let zk_state = self
            .zkvm_state()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        zk_state.serialize(writer)
    }
}

impl BorshDeserialize for HyliUtxoStateExecutor {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let zk_state = HyliUtxoZkVmState::deserialize_reader(reader)?;
        HyliUtxoStateExecutor::from_zkvm_state(zk_state)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }
}

impl HyliUtxoStateExecutor {
    fn from_zkvm_state(state: HyliUtxoZkVmState) -> Result<Self> {
        let mut full = HyliUtxoState::default();
        if !state.notes.values.is_empty() {
            full.record_created(&state.notes.values)
                .map_err(|e| anyhow!(e))?;
        }
        if !state.nullified_notes.values.is_empty() {
            full.record_nullified(&state.nullified_notes.values)
                .map_err(|e| anyhow!(e))?;
        }
        Ok(Self { state: full })
    }

    fn zkvm_state(&self) -> Result<HyliUtxoZkVmState> {
        Ok(self.state.to_zkvm_state())
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
                let zk_state: HyliUtxoZkVmState =
                    borsh::from_slice(bytes).context("decoding HyliUtxoZkVmState")?;
                HyliUtxoStateExecutor::from_zkvm_state(zk_state)?.state
            }
            _ => HyliUtxoState::default(),
        };
        Ok(HyliUtxoStateExecutor { state })
    }

    fn build_commitment_metadata(&self, _blob: &Blob) -> Result<Vec<u8>> {
        borsh::to_vec(&self.zkvm_state()?).context("serializing HyliUtxoZkVmState")
    }

    fn handle(&mut self, calldata: &Calldata) -> Result<HyliOutput> {
        let initial_commitment = self.get_state_commitment();

        let (_, execution_ctx) =
            parse_raw_calldata::<()>(calldata).map_err(|e| anyhow!("parsing calldata: {e}"))?;

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
        self.zkvm_state()
            .expect("conversion to zkvm state")
            .commit()
    }
}
