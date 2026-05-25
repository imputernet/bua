// runtime/src/runtime/trace_ctx.rs
//
// TraceContext is an append-only structured log for one agent execution.
//
// Key properties:
//   - Lock-free reads (event count) via AtomicU64
//   - Append under Mutex (rare contention — only 1 JS thread appends)
//   - Events are immutable once written
//   - NDJSON export for streaming/storage/replay

use bua_core::{AgentId, ExecutionId};
use bua_core::trace::{ExecutionTrace, LogLevel, TraceEventKind};
use parking_lot::Mutex;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Shared, append-only trace for one agent execution.
#[derive(Clone, Debug)]
pub struct TraceContext {
    inner: Arc<Mutex<ExecutionTrace>>,
    event_count: Arc<AtomicU64>,
    enabled: bool,
}

impl TraceContext {
    pub fn new(execution_id: ExecutionId) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ExecutionTrace::new(execution_id))),
            event_count: Arc::new(AtomicU64::new(0)),
            enabled: true,
        }
    }

    pub fn disabled() -> Self {
        let id = ExecutionId::new();
        let mut s = Self::new(id);
        s.enabled = false;
        s
    }

    // --- Emit helpers (zero-cost when disabled) ---

    pub fn execution_start(&self, entrypoint: &str) {
        self.emit(TraceEventKind::ExecutionStart {
            entrypoint: entrypoint.to_string(),
        });
    }

    pub fn execution_end(&self, exit_code: i32, duration_us: u64) {
        self.emit(TraceEventKind::ExecutionEnd { exit_code, duration_us });
    }

    pub fn agent_spawn(&self, agent_id: AgentId, parent_id: Option<AgentId>, entrypoint: &str) {
        self.emit(TraceEventKind::AgentSpawn {
            agent_id,
            parent_id,
            entrypoint: entrypoint.to_string(),
        });
    }

    pub fn tool_call(&self, name: &str, args: &Value) {
        self.emit(TraceEventKind::ToolCall {
            name: name.to_string(),
            args_json: args.to_string(),
        });
    }

    pub fn tool_result(&self, name: &str, result: &Value, duration_us: u64) {
        self.emit(TraceEventKind::ToolResult {
            name: name.to_string(),
            result_json: result.to_string(),
            duration_us,
        });
    }

    pub fn permission_check(&self, operation: &str, granted: bool) {
        self.emit(TraceEventKind::PermissionCheck {
            operation: operation.to_string(),
            granted,
        });
    }

    pub fn js_exception(&self, message: &str, stack: Option<&str>) {
        self.emit(TraceEventKind::JsException {
            message: message.to_string(),
            stack: stack.map(str::to_string),
        });
    }

    pub fn log(&self, level: LogLevel, message: &str) {
        self.emit(TraceEventKind::Log {
            level,
            message: message.to_string(),
        });
    }

    // --- Query ---

    pub fn event_count(&self) -> u64 {
        self.event_count.load(Ordering::Relaxed)
    }

    pub fn to_ndjson(&self) -> String {
        self.inner.lock().to_ndjson()
    }

    /// Export all events as a JSON array (for snapshot embedding).
    pub fn to_json_array(&self) -> Value {
        let ndjson = self.to_ndjson();
        let events: Vec<Value> = ndjson
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        Value::Array(events)
    }

    // --- Internal ---

    fn emit(&self, kind: TraceEventKind) {
        if !self.enabled {
            return;
        }
        self.inner.lock().append(kind);
        self.event_count.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bua_core::ExecutionId;

    #[test]
    fn append_and_count() {
        let trace = TraceContext::new(ExecutionId::new());
        trace.execution_start("test.ts");
        trace.tool_call("bua_read_file", &serde_json::json!({"path": "x"}));
        trace.tool_result("bua_read_file", &serde_json::json!({"content": "y"}), 100);
        assert_eq!(trace.event_count(), 3);
    }

    #[test]
    fn ndjson_is_valid() {
        let trace = TraceContext::new(ExecutionId::new());
        trace.log(LogLevel::Info, "hello");
        let ndjson = trace.to_ndjson();
        let parsed: serde_json::Value = serde_json::from_str(&ndjson).unwrap();
        assert!(parsed.is_object());
    }

    #[test]
    fn disabled_emits_nothing() {
        let trace = TraceContext::disabled();
        trace.execution_start("x.ts");
        trace.log(LogLevel::Error, "oops");
        assert_eq!(trace.event_count(), 0);
    }
}
