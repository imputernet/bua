// runtime/src/deterministic/interceptor.rs
//
// IoInterceptor wraps the tool dispatch layer in deterministic mode.
//
// In deterministic mode, all tool calls are routed through the interceptor:
//   - Reads    → served from ToolStratum (recorded result)
//   - Writes   → silently dropped (no disk mutations during replay)
//   - Network  → served from ToolStratum or blocked
//   - Time     → served from DeterministicClock
//
// This gives a hard guarantee: deterministic replay never mutates state.

use bua_core::{BuaError, BuaResult};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::clock::DeterministicClock;
use super::replay::ReplayEngine;

/// Classification of a tool call's I/O characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoClass {
    /// Pure read — safe to replay from recording.
    Read,
    /// Write — must be suppressed in replay mode.
    Write,
    /// Network — replay from recording; block in strict mode.
    Network,
    /// Pure computation — always allowed.
    Pure,
    /// Time query — redirect to DeterministicClock.
    TimeQuery,
}

/// Classify a tool call by its I/O behavior.
pub fn classify_tool(name: &str, _args: &Value) -> IoClass {
    match name {
        "bua_read_file" => IoClass::Read,
        "bua_write_file" | "bua_delete_file" | "bua_mkdir" => IoClass::Write,
        "bua_http_get" | "bua_http_post" | "bua_fetch" => IoClass::Network,
        "bua_time_now" => IoClass::TimeQuery,
        _ => IoClass::Pure,
    }
}

/// Intercepts tool calls during deterministic execution.
#[derive(Clone, Debug)]
pub struct IoInterceptor {
    replay: Arc<ReplayEngine>,
    clock: DeterministicClock,
    /// True = writes are silently dropped; False = writes error.
    silent_writes: bool,
    /// Whether the interceptor is currently active.
    active: Arc<AtomicBool>,
}

/// Result of an intercepted tool call.
#[derive(Debug)]
pub enum InterceptResult {
    /// Return this recorded value to the caller.
    Replay(Value),
    /// Write suppressed in deterministic mode.
    WriteSuppressed,
    /// Clock value injected.
    ClockInjected(f64),
    /// Pass through to live execution (IoClass::Pure).
    Passthrough,
}

impl IoInterceptor {
    pub fn new(replay: ReplayEngine, clock: DeterministicClock) -> Self {
        Self {
            replay: Arc::new(replay),
            clock,
            silent_writes: true,
            active: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn with_silent_writes(mut self, silent: bool) -> Self {
        self.silent_writes = silent;
        self
    }

    /// Intercept a tool call. Returns how to handle it.
    pub fn intercept(&self, tool_name: &str, args: &Value) -> BuaResult<InterceptResult> {
        if !self.active.load(Ordering::Relaxed) {
            return Ok(InterceptResult::Passthrough);
        }

        match classify_tool(tool_name, args) {
            IoClass::Pure => Ok(InterceptResult::Passthrough),

            IoClass::TimeQuery => {
                let ms = self.clock.now_ms();
                tracing::trace!(tool = tool_name, time_ms = ms, "intercepted time query");
                Ok(InterceptResult::ClockInjected(ms))
            }

            IoClass::Write => {
                tracing::debug!(tool = tool_name, "write suppressed in deterministic mode");
                if self.silent_writes {
                    Ok(InterceptResult::WriteSuppressed)
                } else {
                    Err(BuaError::internal(format!(
                        "write operation '{tool_name}' not allowed in deterministic mode"
                    )))
                }
            }

            IoClass::Read | IoClass::Network => {
                match self.replay.intercept_tool_call(tool_name, args) {
                    Ok(result) => {
                        tracing::trace!(tool = tool_name, "intercepted → replayed");
                        Ok(InterceptResult::Replay(result))
                    }
                    Err(div) => {
                        tracing::error!(divergence = %div, "replay divergence");
                        Err(BuaError::internal(div.to_string()))
                    }
                }
            }
        }
    }

    /// Deactivate the interceptor (switch back to live I/O).
    pub fn deactivate(&self) {
        self.active.store(false, Ordering::Relaxed);
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub fn divergence_count(&self) -> usize {
        self.replay.divergence_count()
    }

    pub fn calls_replayed(&self) -> u64 {
        self.replay.calls_replayed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{LayeredSnapshot, ToolCallRecord, ToolStratum};
    use bua_core::ExecutionId;

    fn make_interceptor(calls: Vec<(&str, &str, &str)>) -> IoInterceptor {
        let mut snap = LayeredSnapshot::new(ExecutionId::new());
        snap.tool = Some(ToolStratum {
            call_log: calls
                .into_iter()
                .enumerate()
                .map(|(i, (n, a, r))| ToolCallRecord {
                    sequence: i as u64,
                    name: n.into(),
                    args_json: a.into(),
                    result_json: r.into(),
                    duration_us: 0,
                    was_error: false,
                })
                .collect(),
        });
        let engine = ReplayEngine::from_snapshot(&snap, false);
        let clock = DeterministicClock::frozen(1_700_000_000_000_000);
        IoInterceptor::new(engine, clock)
    }

    #[test]
    fn read_intercepted_from_recording() {
        let interceptor = make_interceptor(vec![(
            "bua_read_file",
            r#"{"path":"/x"}"#,
            r#"{"content":"hello"}"#,
        )]);
        let result = interceptor
            .intercept("bua_read_file", &serde_json::json!({"path":"/x"}))
            .unwrap();
        assert!(matches!(result, InterceptResult::Replay(_)));
    }

    #[test]
    fn write_suppressed() {
        let interceptor = make_interceptor(vec![]);
        let result = interceptor
            .intercept(
                "bua_write_file",
                &serde_json::json!({"path":"/x","content":"y"}),
            )
            .unwrap();
        assert!(matches!(result, InterceptResult::WriteSuppressed));
    }

    #[test]
    fn pure_passthrough() {
        let interceptor = make_interceptor(vec![]);
        let result = interceptor
            .intercept("bua_json_parse", &serde_json::json!({"text":"{}"}))
            .unwrap();
        assert!(matches!(result, InterceptResult::Passthrough));
    }

    #[test]
    fn time_query_uses_frozen_clock() {
        let interceptor = make_interceptor(vec![]);
        let result = interceptor
            .intercept("bua_time_now", &serde_json::json!({}))
            .unwrap();
        if let InterceptResult::ClockInjected(ms) = result {
            assert_eq!(ms, 1_700_000_000_000_000.0 / 1000.0);
        } else {
            panic!("expected ClockInjected");
        }
    }

    #[test]
    fn deactivated_interceptor_passthroughs_everything() {
        let interceptor = make_interceptor(vec![]);
        interceptor.deactivate();
        let result = interceptor
            .intercept("bua_read_file", &serde_json::json!({"path":"/etc/passwd"}))
            .unwrap();
        assert!(matches!(result, InterceptResult::Passthrough));
    }
}
