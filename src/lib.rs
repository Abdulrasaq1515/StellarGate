pub mod api;
pub mod config;
pub mod db;
pub mod expiry;
pub mod horizon;
pub mod metrics;
pub mod money;
pub mod ssrf;
pub mod strkey;
pub mod webhook;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Tracks background task health: started, stopped, and failure counts.
/// Used for liveness monitoring and alerting on task crashes.
#[derive(Clone)]
pub struct TaskHealth {
    inner: Arc<TaskHealthInner>,
}

struct TaskHealthInner {
    /// Count of task starts.
    started: AtomicU64,
    /// Count of task stops.
    stopped: AtomicU64,
    /// Count of task panics/failures.
    failed: AtomicU64,
}

impl Default for TaskHealthInner {
    fn default() -> Self {
        Self {
            started: AtomicU64::new(0),
            stopped: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        }
    }
}

impl TaskHealth {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TaskHealthInner::default()),
        }
    }

    pub fn task_started(&self) {
        self.inner.started.fetch_add(1, Ordering::Relaxed);
    }

    pub fn task_stopped(&self) {
        self.inner.stopped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn task_failed(&self) {
        self.inner.failed.fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for TaskHealth {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared application state handed to every request handler and the background
/// Horizon poller. Cloning is cheap — the pool and HTTP client are internally
/// reference-counted.
pub struct AppState {
    pub pool: db::Db,
    pub config: config::Config,
    pub http: reqwest::Client,
    pub webhook_http: reqwest::Client,
    /// Webhook delivery metrics: delivered/failed/retried counts and a latency
    /// histogram. Exposed via `GET /metrics` so operators can see delivery
    /// success rate, retry volume, and failure spikes at a glance.
    pub webhook_metrics: metrics::WebhookMetrics,
    /// Auth middleware outcome counters: success/failure (by reason) counts.
    /// Exposed via `GET /metrics` so credential-stuffing or misconfigured
    /// clients are visible without grepping logs.
    pub auth_metrics: metrics::AuthMetrics,
    /// Background task health: tracks started, stopped, and failed task counts
    /// for monitoring and alerting.
    pub task_health: TaskHealth,
}
