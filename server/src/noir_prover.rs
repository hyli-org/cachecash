use std::{sync::Arc, time::Instant};

use anyhow::{anyhow, bail, Context, Result};
use barretenberg::Prove;
use client_sdk::rest_client::NodeApiClient;
use hyli_modules::{
    bus::{BusMessage, SharedMessageBus, LOW_CAPACITY},
    module_bus_client, module_handle_messages,
    modules::Module,
};
use sdk::{Identity, TxHash};
use tokio::task::{JoinError, JoinSet};
use tracing::{error, info};
use zk_primitives::{HyliUtxo, ToBytes, Utxo, HYLI_BLOB_LENGTH_BYTES};

use crate::{
    init::ContractDeployment,
    metrics::FaucetMetrics,
    prover::{NoirProofArtifacts, NoirProver},
};

#[derive(Clone, Debug)]
pub struct HyliUtxoProofJob {
    pub tx_hash: TxHash,
    pub identity: Identity,
    pub utxo: Utxo,
    pub blob: [u8; HYLI_BLOB_LENGTH_BYTES],
    pub tx_blob_count: u32,
    pub blob_index: u32,
}

impl BusMessage for HyliUtxoProofJob {
    const CAPACITY: usize = LOW_CAPACITY;
}

module_bus_client! {
    #[derive(Debug)]
    pub struct HyliUtxoNoirProverBusClient {
        receiver(HyliUtxoProofJob),
    }
}

pub struct HyliUtxoNoirProverCtx {
    pub node: Arc<dyn NodeApiClient + Send + Sync>,
    pub contract: ContractDeployment,
    pub metrics: FaucetMetrics,
}

pub struct HyliUtxoNoirProver {
    bus: HyliUtxoNoirProverBusClient,
    ctx: Arc<HyliUtxoNoirProverCtx>,
    prover: NoirProver,
    metrics: FaucetMetrics,
}

impl Module for HyliUtxoNoirProver {
    type Context = Arc<HyliUtxoNoirProverCtx>;

    async fn build(bus: SharedMessageBus, ctx: Self::Context) -> Result<Self> {
        let bus = HyliUtxoNoirProverBusClient::new_from_bus(bus.new_handle()).await;
        let prover = NoirProver::new();
        let metrics = ctx.metrics.clone();

        Ok(Self {
            bus,
            ctx,
            prover,
            metrics,
        })
    }

    async fn run(&mut self) -> Result<()> {
        let mut proof_tasks = JoinSet::new();

        module_handle_messages! {
            on_self self,
            listen<HyliUtxoProofJob> job => {
                self.metrics.track_noir_job_started();
                let ctx = Arc::clone(&self.ctx);
                let prover = self.prover.clone();
                proof_tasks.spawn(async move { Self::execute_proof_job(ctx, prover, job).await });
            }
            Some(res) = proof_tasks.join_next() => {
                self.handle_task_completion(res);
            }
        };

        Ok(())
    }
}

impl HyliUtxoNoirProver {
    fn handle_task_completion(&self, result: Result<Result<()>, JoinError>) {
        self.metrics.track_noir_job_finished();

        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                error!(
                    error = %err,
                    "failed to prove hyli_utxo transaction, {:#?}",
                    err
                );
            }
            Err(join_err) => {
                error!(
                    error = %join_err,
                    "hyli_utxo proof task terminated unexpectedly"
                );
            }
        }
    }

    async fn execute_proof_job(
        ctx: Arc<HyliUtxoNoirProverCtx>,
        prover: NoirProver,
        job: HyliUtxoProofJob,
    ) -> Result<()> {
        let contract = ctx.contract.clone();
        let contract_name = contract.contract_name.0.clone();
        let hyli_utxo = Self::build_hyli_utxo(&contract_name, &job)?;

        if job.blob_index > job.tx_blob_count {
            bail!(
                "blob index {} out of bounds for transaction with {} blobs",
                job.blob_index,
                job.tx_blob_count
            );
        }

        let tx_hash_str = hex::encode(&job.tx_hash.0);

        let prove_start = Instant::now();
        let proof = hyli_utxo
            .prove()
            .map_err(|err| anyhow!("generating hyli_utxo Noir proof: {err}"))?;
        let prove_duration = prove_start.elapsed();

        info!(
            duration_ms = prove_duration.as_millis(),
            %tx_hash_str,
            "generated hyli_utxo Noir proof"
        );

        let proof_bytes = proof.to_bytes();

        let (proof_tx, _outputs) = prover.build_proof_transaction(
            &contract,
            NoirProofArtifacts {
                proof: proof_bytes,
                program_id: contract.program_id.clone(),
            },
        )?;

        let node = Arc::clone(&ctx.node);
        node.send_tx_proof(proof_tx)
            .await
            .context("submitting hyli_utxo Noir proof to node")?;

        info!(%tx_hash_str, "submitted hyli_utxo Noir proof");

        Ok(())
    }

    pub(crate) fn build_hyli_utxo(contract_name: &str, job: &HyliUtxoProofJob) -> Result<HyliUtxo> {
        let identity_str = job.identity.0.clone();
        if identity_str.len() > u8::MAX as usize {
            bail!("identity '{}' exceeds Noir payload limit", identity_str);
        }

        let padded_identity = pad_right_with_null(&identity_str, 256)?;
        let padded_contract_name = pad_right_with_null(contract_name, 256)?;

        Ok(HyliUtxo {
            version: 1,
            initial_state: [0u8; 4],
            next_state: [0u8; 4],
            identity_len: identity_str.len() as u8,
            identity: padded_identity,
            tx_hash: hex::encode(&job.tx_hash.0),
            index: job.blob_index,
            blob_number: 1,
            blob_index: job.blob_index,
            blob_contract_name_len: contract_name.len() as u8,
            blob_contract_name: padded_contract_name,
            blob_capacity: HYLI_BLOB_LENGTH_BYTES as u32,
            blob_len: HYLI_BLOB_LENGTH_BYTES as u32,
            blob: job.blob,
            tx_blob_count: job.tx_blob_count,
            success: true,
            utxo: job.utxo.clone(),
        })
    }
}

pub(crate) fn pad_right_with_null(value: &str, target_len: usize) -> Result<String> {
    if value.len() > target_len {
        bail!("string '{}' exceeds maximum length {}", value, target_len);
    }
    let mut padded = String::with_capacity(target_len);
    padded.push_str(value);
    if value.len() < target_len {
        padded.extend(std::iter::repeat('\0').take(target_len - value.len()));
    }
    Ok(padded)
}
