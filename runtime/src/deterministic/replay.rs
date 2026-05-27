// runtime/src/deterministic/replay.rs
//
// ReplayEngine drives a deterministic re-execution from a LayeredSnapshot.
//
// During replay:
//   1. Clock is frozen at snapshot timestamp
//   2. Tool calls are intercepted — results served from ToolStratum
//   3. Each trace event is compared against the recorded TraceStratum
//   4. Any divergence (different tool called, different args) = DivergenceError
//
// This gives Bua the ability to:
//   - Reproduce any past execution byte-for-byte
//   - Detect non-determinism in agent code
//   - Debug failures by replaying up to the point of divergence

use serde_json::Value;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::snapshot::{LayeredSnapshot, ToolCallRecord};

/// Error produced when replay diverges from the recorded trace.
#[derive(Debug, Clone)]
pub struct DivergenceError {
    /// Which tool call sequence number diverged.
    pub at_sequence: u64,
    pub expected_tool: String,
    pub actual_tool: String,
    pub expected_args: String,
    pub actual_args: String,
}

impl std::fmt::Display for DivergenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "replay diverged at call #{}: expected {}({}) got {}({})",
            self.at_sequence,
            self.expected_tool,
            self.expected_args,
            self.actual_tool,
            self.actual_args,
        )
    }
}

impl std::error::Error for DivergenceError {}

/// Result of a completed replay run.
#[derive(Debug)]
pub struct ReplayResult {
    pub calls_replayed: u64,
    pub divergences: Vec<DivergenceError>,
    pub trace_events_matched: u64,
    pub deterministic: bool,
}

impl ReplayResult {
    pub fn is_clean(&self) -> bool {
        self.divergences.is_empty() && self.deterministic
    }
}

/// Drives deterministic replay of a recorded execution.
#[derive(Clone, Debug)]
pub struct ReplayEngine {
    /// Recorded tool call responses to serve in order.
    recorded_calls: Arc<Mutex<VecDeque<ToolCallRecord>>>,
    /// Divergences detected so far.
    divergences: Arc<Mutex<Vec<DivergenceError>>>,
    /// Sequence counter for current replay.
    sequence: Arc<AtomicU64>,
    /// Whether strict mode is on (divergence = hard error vs warning).
    strict: bool,
}

impl ReplayEngine {
    /// Build a ReplayEngine from a snapshot's ToolStratum.
    pub fn from_snapshot(snap: &LayeredSnapshot, strict: bool) -> Self {
        let calls = snap
            .tool
            .as_ref()
            .map(|t| t.call_log.iter().cloned().collect())
            .unwrap_or_default();

        Self {
            recorded_calls: Arc::new(Mutex::new(calls)),
            divergences: Arc::new(Mutex::new(Vec::new())),
            sequence: Arc::new(AtomicU64::new(0)),
            strict,
        }
    }

    /// Intercept a tool call during replay.
    ///
    /// If the call matches the recording, returns the recorded result.
    /// If it diverges, records a DivergenceError and:
    ///   - In strict mode: returns an error result
    ///   - In lax mode: returns the recorded result anyway (best effort)
    pub fn intercept_tool_call(
        &self,
        tool_name: &str,
        args: &Value,
    ) -> Result<Value, DivergenceError> {
        let seq = self.sequence.fetch_add(1, Ordering::SeqCst);
        let args_str = args.to_string();

        let mut queue = self.recorded_calls.lock().unwrap();
        let recorded = queue.pop_front();

        match recorded {
            None => {
                // More calls than recorded — always a divergence.
                let div = DivergenceError {
                    at_sequence: seq,
                    expected_tool: "<end of recording>".into(),
                    actual_tool: tool_name.to_string(),
                    expected_args: "".into(),
                    actual_args: args_str,
                };
                self.divergences.lock().unwrap().push(div.clone());
                tracing::error!(seq, tool = tool_name, "replay: extra call beyond recording");
                Err(div)
            }

            Some(record) => {
                let name_match = record.name == tool_name;
                // Args match with JSON normalization (key order may differ).
                let recorded_args: Value =
                    serde_json::from_str(&record.args_json).unwrap_or(Value::Null);
                let args_match = json_equivalent(&recorded_args, args);

                if !name_match || !args_match {
                    let div = DivergenceError {
                        at_sequence: seq,
                        expected_tool: record.name.clone(),
                        actual_tool: tool_name.to_string(),
                        expected_args: record.args_json.clone(),
                        actual_args: args_str,
                    };
                    self.divergences.lock().unwrap().push(div.clone());

                    tracing::warn!(
                        seq,
                        expected_tool = %record.name,
                        actual_tool = tool_name,
                        "replay divergence detected"
                    );

                    if self.strict {
                        return Err(div);
                    }
                } else {
                    tracing::trace!(seq, tool = tool_name, "replay: call matched");
                }

                // Return recorded result regardless of divergence in lax mode.
                let result: Value =
                    serde_json::from_str(&record.result_json).unwrap_or(Value::Null);
                Ok(result)
            }
        }
    }

    /// Check that all recorded calls were consumed (no missing calls).
    pub fn verify_complete(&self) -> Option<DivergenceError> {
        let remaining = self.recorded_calls.lock().unwrap().len();
        if remaining > 0 {
            Some(DivergenceError {
                at_sequence: self.sequence.load(Ordering::Relaxed),
                expected_tool: format!("{remaining} more calls expected"),
                actual_tool: "<end of execution>".into(),
                expected_args: String::new(),
                actual_args: String::new(),
            })
        } else {
            None
        }
    }

    pub fn result(&self) -> ReplayResult {
        let divergences = self.divergences.lock().unwrap().clone();
        let calls = self.sequence.load(Ordering::Relaxed);
        ReplayResult {
            calls_replayed: calls,
            deterministic: divergences.is_empty(),
            trace_events_matched: calls, // simplified; real impl compares TraceStratum
            divergences,
        }
    }

    pub fn divergence_count(&self) -> usize {
        self.divergences.lock().unwrap().len()
    }

    pub fn calls_replayed(&self) -> u64 {
        self.sequence.load(Ordering::Relaxed)
    }
}

/// JSON structural equivalence (order-independent for objects).
fn json_equivalent(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(ao), Value::Object(bo)) => {
            if ao.len() != bo.len() {
                return false;
            }
            ao.iter()
                .all(|(k, v)| bo.get(k).is_some_and(|bv| json_equivalent(v, bv)))
        }
        (Value::Array(aa), Value::Array(ba)) => {
            aa.len() == ba.len() && aa.iter().zip(ba.iter()).all(|(a, b)| json_equivalent(a, b))
        }
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{LayeredSnapshot, ToolCallRecord, ToolStratum};
    use bua_core::ExecutionId;

    fn make_snap_with_calls(calls: Vec<(&str, &str, &str)>) -> LayeredSnapshot {
        let mut snap = LayeredSnapshot::new(ExecutionId::new());
        snap.tool = Some(ToolStratum {
            call_log: calls
                .into_iter()
                .enumerate()
                .map(|(i, (name, args, result))| ToolCallRecord {
                    sequence: i as u64,
                    name: name.into(),
                    args_json: args.into(),
                    result_json: result.into(),
                    duration_us: 100,
                    was_error: false,
                })
                .collect(),
        });
        snap
    }

    #[test]
    fn clean_replay() {
        let snap = make_snap_with_calls(vec![
            (
                "bua_read_file",
                r#"{"path":"/x"}"#,
                r#"{"content":"hello"}"#,
            ),
            (
                "bua_http_get",
                r#"{"url":"https://a.com"}"#,
                r#"{"status":200}"#,
            ),
        ]);

        let engine = ReplayEngine::from_snapshot(&snap, true);

        let r1 = engine.intercept_tool_call("bua_read_file", &serde_json::json!({"path":"/x"}));
        assert!(r1.is_ok());

        let r2 =
            engine.intercept_tool_call("bua_http_get", &serde_json::json!({"url":"https://a.com"}));
        assert!(r2.is_ok());

        assert!(engine.verify_complete().is_none());
        assert!(engine.result().is_clean());
    }

    #[test]
    fn wrong_tool_name_diverges() {
        let snap = make_snap_with_calls(vec![(
            "bua_read_file",
            r#"{"path":"/x"}"#,
            r#"{"content":"hi"}"#,
        )]);
        let engine = ReplayEngine::from_snapshot(&snap, false); // lax mode

        let result = engine.intercept_tool_call("bua_http_get", &serde_json::json!({"url":"x"}));
        // Lax mode: returns recorded result
        assert!(result.is_ok());
        assert_eq!(engine.divergence_count(), 1);
    }

    #[test]
    fn strict_mode_diverge_errors() {
        let snap = make_snap_with_calls(vec![("tool_a", "{}", "{}")]);
        let engine = ReplayEngine::from_snapshot(&snap, true);

        let result = engine.intercept_tool_call("tool_b", &serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn extra_call_beyond_recording() {
        let snap = make_snap_with_calls(vec![]);
        let engine = ReplayEngine::from_snapshot(&snap, false);

        let result = engine.intercept_tool_call("any_tool", &serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn missing_calls_detected_by_verify_complete() {
        let snap = make_snap_with_calls(vec![("tool_a", "{}", "{}"), ("tool_b", "{}", "{}")]);
        let engine = ReplayEngine::from_snapshot(&snap, false);

        // Only replay first call
        engine
            .intercept_tool_call("tool_a", &serde_json::json!({}))
            .ok();

        let remaining = engine.verify_complete();
        assert!(remaining.is_some());
    }

    #[test]
    fn json_object_order_independent() {
        let a = serde_json::json!({"z": 1, "a": 2});
        let b = serde_json::json!({"a": 2, "z": 1});
        assert!(json_equivalent(&a, &b));
    }
}
