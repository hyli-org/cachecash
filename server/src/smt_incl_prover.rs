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
use zk_primitives::{InputNote, HyliSmtIncl, HyliSmtInclProof, ToBytes, HYLI_SMT_INCL_BLOB_LENGTH_BYTES};

use crate::{
    init::ContractDeployment,
    prover::{NoirProofArtifacts, NoirProver},
};

#[derive(Clone, Debug)]
pub struct SmtInclProofJob {
    pub tx_hash: TxHash,
    pub identity: Identity,
    pub blob: [u8; HYLI_SMT_INCL_BLOB_LENGTH_BYTES],
    pub tx_blob_count: u32,
    pub blob_index: u32,
    pub input_notes: [InputNote; 2],
    pub siblings_0: Box<[element::Base; 256]>,
    pub siblings_1: Box<[element::Base; 256]>,
}

impl BusMessage for SmtInclProofJob {
    const CAPACITY: usize = LOW_CAPACITY;
}

module_bus_client! {
    #[derive(Debug)]
    pub struct SmtInclProverBusClient {
        receiver(SmtInclProofJob),
    }
}

pub struct SmtInclProverCtx {
    pub node: Arc<dyn NodeApiClient + Send + Sync>,
    pub contract: ContractDeployment,
}

pub struct HyliSmtInclNoirProver {
    bus: SmtInclProverBusClient,
    ctx: Arc<SmtInclProverCtx>,
    prover: NoirProver,
}

impl Module for HyliSmtInclNoirProver {
    type Context = Arc<SmtInclProverCtx>;

    async fn build(bus: SharedMessageBus, ctx: Self::Context) -> Result<Self> {
        let bus = SmtInclProverBusClient::new_from_bus(bus.new_handle()).await;
        let prover = NoirProver::default();

        Ok(Self { bus, ctx, prover })
    }

    async fn run(&mut self) -> Result<()> {
        let mut proof_tasks = JoinSet::new();

        module_handle_messages! {
            on_self self,
            listen<SmtInclProofJob> job => {
                let ctx = Arc::clone(&self.ctx);
                let prover = self.prover.clone();
                proof_tasks.spawn(async move { Self::execute_proof_job(ctx, prover, job).await });
            }
            Some(res) = proof_tasks.join_next() => {
                Self::handle_task_completion(res);
            }
        };

        Ok(())
    }
}

impl HyliSmtInclNoirProver {
    fn handle_task_completion(result: Result<Result<()>, JoinError>) {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                error!(
                    error = %err,
                    "failed to prove hyli_smt_incl_proof transaction, {:#?}",
                    err
                );
            }
            Err(join_err) => {
                error!(
                    error = %join_err,
                    "hyli_smt_incl_proof task terminated unexpectedly"
                );
            }
        }
    }

    async fn execute_proof_job(
        ctx: Arc<SmtInclProverCtx>,
        prover: NoirProver,
        job: SmtInclProofJob,
    ) -> Result<()> {
        let contract = ctx.contract.clone();
        let contract_name = contract.contract_name.0.clone();
        let hyli_smt_incl = Self::build_hyli_smt_incl(&contract_name, &job)?;

        if job.blob_index > job.tx_blob_count {
            bail!(
                "blob index {} out of bounds for transaction with {} blobs",
                job.blob_index,
                job.tx_blob_count
            );
        }

        let tx_hash_str = hex::encode(&job.tx_hash.0);

        let prove_start = Instant::now();
        let proof: HyliSmtInclProof = hyli_smt_incl
            .prove()
            .map_err(|err| anyhow!("generating hyli_smt_incl_proof Noir proof: {err}"))?;
        let prove_duration = prove_start.elapsed();

        info!(
            duration_ms = prove_duration.as_millis(),
            %tx_hash_str,
            "generated hyli_smt_incl_proof Noir proof"
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
            .context("submitting hyli_smt_incl_proof Noir proof to node")?;

        info!(%tx_hash_str, "submitted hyli_smt_incl_proof Noir proof");

        Ok(())
    }

    pub(crate) fn build_hyli_smt_incl(
        contract_name: &str,
        job: &SmtInclProofJob,
    ) -> Result<HyliSmtIncl> {
        let identity_str = job.identity.0.clone();
        if identity_str.len() > u8::MAX as usize {
            bail!("identity '{}' exceeds Noir payload limit", identity_str);
        }

        let padded_identity = pad_right_with_null(&identity_str, 256)?;
        let padded_contract_name = pad_right_with_null(contract_name, 256)?;

        Ok(HyliSmtIncl {
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
            blob_capacity: HYLI_SMT_INCL_BLOB_LENGTH_BYTES as u32,
            blob_len: HYLI_SMT_INCL_BLOB_LENGTH_BYTES as u32,
            blob: job.blob,
            tx_blob_count: job.tx_blob_count,
            success: true,
            input_notes: job.input_notes.clone(),
            siblings_0: job.siblings_0.clone(),
            siblings_1: job.siblings_1.clone(),
        })
    }
}

fn pad_right_with_null(value: &str, target_len: usize) -> Result<String> {
    if value.len() > target_len {
        bail!("string '{}' exceeds maximum length {}", value, target_len);
    }
    let mut padded = String::with_capacity(target_len);
    padded.push_str(value);
    if value.len() < target_len {
        padded.extend(std::iter::repeat_n('\0', target_len - value.len()));
    }
    Ok(padded)
}
