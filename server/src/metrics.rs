use hyli_modules::telemetry::{Counter, Gauge, Histogram, KeyValue};
use hyli_turmoil_shims::global_meter_or_panic;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

#[derive(Clone)]
pub struct FaucetMetrics {
    requests_total: Counter<u64>,
    requests_failed: Counter<u64>,
    minted_amount: Histogram<u64>,
    noir_jobs_gauge: Gauge<u64>,
    base_labels: Vec<KeyValue>,
    current_noir_jobs: Arc<AtomicU64>,
}

impl FaucetMetrics {
    pub fn global(node_id: String) -> Self {
        let meter = global_meter_or_panic();
        let base_labels = vec![KeyValue::new("node_id", node_id)];

        Self {
            requests_total: meter.u64_counter("faucet_requests_total").build(),
            requests_failed: meter.u64_counter("faucet_requests_failed_total").build(),
            minted_amount: meter.u64_histogram("faucet_minted_amount").build(),
            base_labels,
            noir_jobs_gauge: meter.u64_gauge("faucet_noir_jobs_inflight").build(),
            current_noir_jobs: Arc::new(AtomicU64::new(0)),
        }
    }

    fn base(&self) -> Vec<KeyValue> {
        self.base_labels.clone()
    }

    pub fn record_success(&self, amount: u64) {
        let mut labels = self.base();
        labels.push(KeyValue::new("status", "success"));
        self.requests_total.add(1, &labels);

        self.minted_amount.record(amount, &labels);
    }

    pub fn record_failure(&self, reason: &'static str) {
        let mut labels = self.base();
        labels.push(KeyValue::new("status", "failure"));
        self.requests_total.add(1, &labels);

        let mut failure_labels = self.base();
        failure_labels.push(KeyValue::new("reason", reason));
        self.requests_failed.add(1, &failure_labels);
    }

    pub fn track_noir_job_started(&self) {
        let current = self.current_noir_jobs.fetch_add(1, Ordering::Relaxed) + 1;
        self.noir_jobs_gauge.record(current, &self.base());
    }

    pub fn track_noir_job_finished(&self) {
        let mut current = self.current_noir_jobs.load(Ordering::Relaxed);

        loop {
            if current == 0 {
                self.noir_jobs_gauge.record(0, &self.base());
                return;
            }

            match self.current_noir_jobs.compare_exchange(
                current,
                current - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.noir_jobs_gauge.record(current - 1, &self.base());
                    return;
                }
                Err(next) => current = next,
            }
        }
    }
}
