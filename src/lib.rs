pub mod api;
pub mod config;
pub mod db;
pub mod expiry;
pub mod horizon;
pub mod metrics;
pub mod money;
pub mod retention;
pub mod ssrf;
pub mod strkey;
pub mod webhook;

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Tracks background task health: per-task liveness for `/health`, the last
/// successful on-chain progress for `/ready`'s cursor-freshness check, and
/// started/stopped/failure counts for monitoring and alerting.
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
    /// Per-task liveness, keyed by the name passed to [`TaskHealth::task_started`].
    running: Mutex<HashMap<&'static str, bool>>,
    /// Task names that must be running for `/health` to pass. Declared by the
    /// process that spawns the tasks (main.rs), so "expected" is a deployment
    /// decision rather than something the probe has to guess.
    required: Mutex<Vec<&'static str>>,
    /// Unix seconds of the last successful poll cycle or stream event; `0`
    /// means never. Drives `/ready`'s cursor-freshness check (issue #315).
    last_success_unix: AtomicI64,
}

impl Default for TaskHealthInner {
    fn default() -> Self {
        Self {
            started: AtomicU64::new(0),
            stopped: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            running: Mutex::new(HashMap::new()),
            required: Mutex::new(Vec::new()),
            last_success_unix: AtomicI64::new(0),
        }
    }
}

impl TaskHealth {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TaskHealthInner::default()),
        }
    }

    /// Declare that the named task must keep running for the service to be
    /// healthy. `/health` returns `503` while any required task is not running
    /// (issue #315). Call before the task is spawned.
    pub fn require(&self, name: &'static str) {
        self.inner.required.lock().unwrap().push(name);
    }

    pub fn task_started(&self, name: &'static str) {
        self.inner.started.fetch_add(1, Ordering::Relaxed);
        self.inner.running.lock().unwrap().insert(name, true);
    }

    pub fn task_stopped(&self, name: &'static str) {
        self.inner.stopped.fetch_add(1, Ordering::Relaxed);
        self.inner.running.lock().unwrap().insert(name, false);
    }

    pub fn task_failed(&self, name: &'static str) {
        self.inner.failed.fetch_add(1, Ordering::Relaxed);
        // A failed task is by definition not running; reflect that in the
        // liveness map even if `task_stopped` never ran for it.
        self.inner.running.lock().unwrap().insert(name, false);
    }

    /// Names of required tasks that are not currently running. Empty when the
    /// service is healthy; drives `/health`.
    pub fn dead_required_tasks(&self) -> Vec<&'static str> {
        let running = self.inner.running.lock().unwrap();
        let required = self.inner.required.lock().unwrap();
        required
            .iter()
            .copied()
            .filter(|name| running.get(name) != Some(&true))
            .collect()
    }

    /// Record successful on-chain progress — a completed poll cycle or a
    /// received stream event. This is the heartbeat `/ready` freshness-checks.
    pub fn note_success(&self) {
        self.set_last_success_unix(unix_now_secs());
    }

    /// Overwrite the last-success timestamp directly. Used by tests to
    /// simulate a stale cursor without waiting out a poll interval.
    pub fn set_last_success_unix(&self, unix_secs: i64) {
        self.inner
            .last_success_unix
            .store(unix_secs, Ordering::Relaxed);
    }

    /// Seconds since the last successful poll/stream event. A never-updated
    /// timestamp (`0`) reads as maximally stale; a clock before the epoch
    /// saturates at `0` rather than going negative.
    pub fn last_success_age_secs(&self) -> i64 {
        unix_now_secs().saturating_sub(self.inner.last_success_unix.load(Ordering::Relaxed))
    }
}

/// Current Unix time in whole seconds.
fn unix_now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
    /// Background task health: per-task liveness (drives `/health`), the last
    /// successful on-chain progress (drives `/ready`'s cursor-freshness check),
    /// and started/stopped/failure counts for monitoring and alerting.
    pub task_health: TaskHealth,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_required_tasks_is_healthy() {
        let health = TaskHealth::new();
        assert!(health.dead_required_tasks().is_empty());
    }

    #[test]
    fn running_required_task_is_healthy() {
        let health = TaskHealth::new();
        health.require("poller");
        health.task_started("poller");
        assert!(health.dead_required_tasks().is_empty());
    }

    #[test]
    fn stopped_required_task_is_dead() {
        let health = TaskHealth::new();
        health.require("poller");
        health.task_started("poller");
        health.task_stopped("poller");
        assert_eq!(health.dead_required_tasks(), vec!["poller"]);
    }

    #[test]
    fn never_started_required_task_is_dead() {
        let health = TaskHealth::new();
        health.require("redrive");
        assert_eq!(health.dead_required_tasks(), vec!["redrive"]);
    }

    #[test]
    fn unrequired_stopped_task_does_not_fail_health() {
        // A task that is not required (e.g. the stream on a poll-only
        // deployment, or the poller without a configured gateway) stopping is
        // by design and must not fail /health.
        let health = TaskHealth::new();
        health.require("sweeper");
        health.task_started("sweeper");
        health.task_started("poller");
        health.task_stopped("poller");
        assert!(health.dead_required_tasks().is_empty());
    }

    #[test]
    fn failed_task_is_not_running() {
        // task_failed marks the task dead even if task_stopped never ran.
        let health = TaskHealth::new();
        health.require("poller");
        health.task_started("poller");
        health.task_failed("poller");
        assert_eq!(health.dead_required_tasks(), vec!["poller"]);
    }

    #[test]
    fn last_success_age_is_zero_after_note_success() {
        let health = TaskHealth::new();
        health.note_success();
        assert!(health.last_success_age_secs() <= 1);
    }

    #[test]
    fn never_succeeded_is_maximally_stale() {
        let health = TaskHealth::new();
        assert!(health.last_success_age_secs() > 1_000_000_000);
    }

    #[test]
    fn stale_timestamp_reports_large_age() {
        let health = TaskHealth::new();
        health.set_last_success_unix(unix_now_secs() - 120);
        assert!(health.last_success_age_secs() >= 119);
    }
}
