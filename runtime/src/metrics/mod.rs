// runtime/src/metrics/mod.rs
//
// Runtime observability metrics.
// All metrics are atomic — zero locking, cheap to read from any thread.
// Exported as NDJSON, structured JSON, or human-readable text.

use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Atomic histogram (lock-free approximate percentiles via bucket counting)
// ---------------------------------------------------------------------------

/// A simple fixed-bucket histogram for latency tracking.
#[derive(Debug)]
pub struct Histogram {
    /// Bucket boundaries in microseconds: [0, 100, 1000, 10000, 100000, inf]
    buckets: [AtomicU64; 6],
    sum_us: AtomicU64,
    count: AtomicU64,
}

const BUCKET_BOUNDS_US: [u64; 5] = [100, 1_000, 10_000, 100_000, 1_000_000];

impl Histogram {
    pub fn new() -> Self {
        Self {
            buckets: Default::default(),
            sum_us: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    pub fn record(&self, duration_us: u64) {
        self.sum_us.fetch_add(duration_us, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);

        let bucket = BUCKET_BOUNDS_US
            .iter()
            .position(|&b| duration_us < b)
            .unwrap_or(5);
        self.buckets[bucket].fetch_add(1, Ordering::Relaxed);
    }

    pub fn count(&self) -> u64 { self.count.load(Ordering::Relaxed) }

    pub fn mean_us(&self) -> f64 {
        let c = self.count();
        if c == 0 { 0.0 } else { self.sum_us.load(Ordering::Relaxed) as f64 / c as f64 }
    }

    pub fn to_json(&self) -> serde_json::Value {
        let buckets: Vec<u64> = self.buckets.iter()
            .map(|b| b.load(Ordering::Relaxed))
            .collect();
        serde_json::json!({
            "count": self.count(),
            "mean_us": self.mean_us(),
            "buckets": {
                "<100µs":  buckets[0],
                "<1ms":    buckets[1],
                "<10ms":   buckets[2],
                "<100ms":  buckets[3],
                "<1s":     buckets[4],
                ">=1s":    buckets[5],
            }
        })
    }
}

impl Default for Histogram { fn default() -> Self { Self::new() } }

// ---------------------------------------------------------------------------
// RuntimeMetrics — the top-level metrics object
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct RuntimeMetrics {
    // --- Execution ---
    pub agents_started:   AtomicU64,
    pub agents_completed: AtomicU64,
    pub agents_failed:    AtomicU64,
    pub agents_timed_out: AtomicU64,
    pub agents_active:    AtomicI64,

    // --- Tool calls ---
    pub tool_calls_total:  AtomicU64,
    pub tool_calls_failed: AtomicU64,
    pub tool_latency:      Histogram,

    // --- Promise bridge ---
    pub promises_created:  AtomicU64,
    pub promises_resolved: AtomicU64,
    pub promises_rejected: AtomicU64,
    pub promises_pending:  AtomicI64,

    // --- Module loader ---
    pub modules_loaded:       AtomicU64,
    pub modules_cache_hits:   AtomicU64,
    pub modules_transpiled:   AtomicU64,
    pub module_load_latency:  Histogram,

    // --- Snapshots ---
    pub snapshots_written: AtomicU64,
    pub snapshots_loaded:  AtomicU64,
    pub snapshot_bytes:    AtomicU64,

    // --- JS engine ---
    pub eval_calls:    AtomicU64,
    pub eval_latency:  Histogram,
    pub microtask_drains: AtomicU64,

    // --- Permission checks ---
    pub permission_checks_allowed: AtomicU64,
    pub permission_checks_denied:  AtomicU64,

    // --- Process start time ---
    #[allow(dead_code)]
    start_time: Instant,
}

impl RuntimeMetrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            start_time: Instant::now(),
            ..Default::default()
        })
    }

    // --- Agent ---
    pub fn agent_started(&self) {
        self.agents_started.fetch_add(1, Ordering::Relaxed);
        self.agents_active.fetch_add(1, Ordering::Relaxed);
    }
    pub fn agent_completed(&self) {
        self.agents_completed.fetch_add(1, Ordering::Relaxed);
        self.agents_active.fetch_sub(1, Ordering::Relaxed);
    }
    pub fn agent_failed(&self) {
        self.agents_failed.fetch_add(1, Ordering::Relaxed);
        self.agents_active.fetch_sub(1, Ordering::Relaxed);
    }
    pub fn agent_timed_out(&self) {
        self.agents_timed_out.fetch_add(1, Ordering::Relaxed);
        self.agents_active.fetch_sub(1, Ordering::Relaxed);
    }

    // --- Tools ---
    pub fn tool_call_completed(&self, duration_us: u64) {
        self.tool_calls_total.fetch_add(1, Ordering::Relaxed);
        self.tool_latency.record(duration_us);
    }
    pub fn tool_call_failed(&self) {
        self.tool_calls_failed.fetch_add(1, Ordering::Relaxed);
    }

    // --- Promises ---
    pub fn promise_created(&self) {
        self.promises_created.fetch_add(1, Ordering::Relaxed);
        self.promises_pending.fetch_add(1, Ordering::Relaxed);
    }
    pub fn promise_resolved(&self) {
        self.promises_resolved.fetch_add(1, Ordering::Relaxed);
        self.promises_pending.fetch_sub(1, Ordering::Relaxed);
    }
    pub fn promise_rejected(&self) {
        self.promises_rejected.fetch_add(1, Ordering::Relaxed);
        self.promises_pending.fetch_sub(1, Ordering::Relaxed);
    }

    // --- Modules ---
    pub fn module_loaded(&self, was_cache_hit: bool, was_transpiled: bool, duration_us: u64) {
        self.modules_loaded.fetch_add(1, Ordering::Relaxed);
        if was_cache_hit { self.modules_cache_hits.fetch_add(1, Ordering::Relaxed); }
        if was_transpiled { self.modules_transpiled.fetch_add(1, Ordering::Relaxed); }
        self.module_load_latency.record(duration_us);
    }

    // --- Eval ---
    pub fn eval_completed(&self, duration_us: u64) {
        self.eval_calls.fetch_add(1, Ordering::Relaxed);
        self.eval_latency.record(duration_us);
    }

    // --- Permissions ---
    pub fn permission_allowed(&self) { self.permission_checks_allowed.fetch_add(1, Ordering::Relaxed); }
    pub fn permission_denied(&self)  { self.permission_checks_denied.fetch_add(1, Ordering::Relaxed); }

    /// Export all metrics as a single JSON object.
    pub fn to_json(&self) -> serde_json::Value {
        let uptime_s = self.start_time.elapsed().as_secs_f64();
        serde_json::json!({
            "uptime_s": uptime_s,
            "agents": {
                "started":   self.agents_started.load(Ordering::Relaxed),
                "completed": self.agents_completed.load(Ordering::Relaxed),
                "failed":    self.agents_failed.load(Ordering::Relaxed),
                "timed_out": self.agents_timed_out.load(Ordering::Relaxed),
                "active":    self.agents_active.load(Ordering::Relaxed),
            },
            "tools": {
                "calls_total":  self.tool_calls_total.load(Ordering::Relaxed),
                "calls_failed": self.tool_calls_failed.load(Ordering::Relaxed),
                "latency":      self.tool_latency.to_json(),
            },
            "promises": {
                "created":  self.promises_created.load(Ordering::Relaxed),
                "resolved": self.promises_resolved.load(Ordering::Relaxed),
                "rejected": self.promises_rejected.load(Ordering::Relaxed),
                "pending":  self.promises_pending.load(Ordering::Relaxed),
            },
            "modules": {
                "loaded":       self.modules_loaded.load(Ordering::Relaxed),
                "cache_hits":   self.modules_cache_hits.load(Ordering::Relaxed),
                "transpiled":   self.modules_transpiled.load(Ordering::Relaxed),
                "load_latency": self.module_load_latency.to_json(),
            },
            "eval": {
                "calls":           self.eval_calls.load(Ordering::Relaxed),
                "latency":         self.eval_latency.to_json(),
                "microtask_drains": self.microtask_drains.load(Ordering::Relaxed),
            },
            "snapshots": {
                "written": self.snapshots_written.load(Ordering::Relaxed),
                "loaded":  self.snapshots_loaded.load(Ordering::Relaxed),
                "bytes":   self.snapshot_bytes.load(Ordering::Relaxed),
            },
            "permissions": {
                "allowed": self.permission_checks_allowed.load(Ordering::Relaxed),
                "denied":  self.permission_checks_denied.load(Ordering::Relaxed),
            }
        })
    }

    /// Export as NDJSON event (for streaming pipelines).
    pub fn to_ndjson_event(&self) -> String {
        let mut val = self.to_json();
        val["type"] = serde_json::json!("metrics");
        val["timestamp_us"] = serde_json::json!(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64
        );
        serde_json::to_string(&val).unwrap_or_default()
    }

    /// Human-readable summary for CLI output.
    pub fn summary(&self) -> String {
        format!(
            "agents: {} started, {} active, {} failed | \
             tools: {} calls ({} failed, {:.1}µs avg) | \
             modules: {} loaded ({} cached) | \
             perms: {} allowed, {} denied",
            self.agents_started.load(Ordering::Relaxed),
            self.agents_active.load(Ordering::Relaxed),
            self.agents_failed.load(Ordering::Relaxed),
            self.tool_calls_total.load(Ordering::Relaxed),
            self.tool_calls_failed.load(Ordering::Relaxed),
            self.tool_latency.mean_us(),
            self.modules_loaded.load(Ordering::Relaxed),
            self.modules_cache_hits.load(Ordering::Relaxed),
            self.permission_checks_allowed.load(Ordering::Relaxed),
            self.permission_checks_denied.load(Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_records_correctly() {
        let h = Histogram::new();
        h.record(50);    // <100µs bucket
        h.record(500);   // <1ms bucket
        h.record(5_000); // <10ms bucket
        assert_eq!(h.count(), 3);
        let mean = h.mean_us();
        assert!((mean - 1850.0).abs() < 1.0);
    }

    #[test]
    fn metrics_json_has_all_keys() {
        let m = RuntimeMetrics::new();
        m.agent_started();
        m.tool_call_completed(1000);
        m.promise_created();
        m.promise_resolved();
        m.module_loaded(false, true, 500);
        m.eval_completed(200);
        m.permission_allowed();
        m.permission_denied();

        let json = m.to_json();
        assert!(json["agents"]["started"].as_u64() == Some(1));
        assert!(json["tools"]["calls_total"].as_u64() == Some(1));
        assert!(json["modules"]["transpiled"].as_u64() == Some(1));
        assert!(json["permissions"]["denied"].as_u64() == Some(1));
    }

    #[test]
    fn ndjson_event_is_parseable() {
        let m = RuntimeMetrics::new();
        let ndjson = m.to_ndjson_event();
        let v: serde_json::Value = serde_json::from_str(&ndjson).unwrap();
        assert_eq!(v["type"], "metrics");
    }

    #[test]
    fn agent_active_count_tracks_correctly() {
        let m = RuntimeMetrics::new();
        m.agent_started();
        m.agent_started();
        assert_eq!(m.agents_active.load(Ordering::Relaxed), 2);
        m.agent_completed();
        assert_eq!(m.agents_active.load(Ordering::Relaxed), 1);
        m.agent_failed();
        assert_eq!(m.agents_active.load(Ordering::Relaxed), 0);
    }
}
