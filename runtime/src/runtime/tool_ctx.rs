// runtime/src/runtime/tool_ctx.rs
//
// ToolContext wraps ToolRegistry with:
//   - Capability enforcement before dispatch
//   - Per-call trace emission
//   - Rate limiting hooks (Phase 3)
//   - Tool call history for replay

use bua_core::BuaResult;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::capability_ctx::CapabilityContext;
use super::trace_ctx::TraceContext;
use crate::tools::{ToolCall, ToolRegistry};

/// Per-agent tool dispatch context.
#[derive(Clone, Debug)]
pub struct ToolContext {
    registry: Arc<ToolRegistry>,
    caps: CapabilityContext,
    trace: TraceContext,
    /// Monotonic call counter for this agent.
    call_count: Arc<AtomicU64>,
    /// Optional call rate limit (calls/second). None = unlimited.
    rate_limit: Option<f64>,
}

impl ToolContext {
    pub fn new(registry: Arc<ToolRegistry>, caps: CapabilityContext, trace: TraceContext) -> Self {
        Self {
            registry,
            caps,
            trace,
            call_count: Arc::new(AtomicU64::new(0)),
            rate_limit: None,
        }
    }

    pub fn with_rate_limit(mut self, calls_per_second: f64) -> Self {
        self.rate_limit = Some(calls_per_second);
        self
    }

    /// Dispatch a tool call, enforcing capabilities and emitting trace events.
    pub async fn call(&self, name: &str, args: Value) -> BuaResult<Value> {
        let call = ToolCall {
            name: name.to_string(),
            args: args.clone(),
            call_id: Some(format!(
                "tc-{}",
                self.call_count.fetch_add(1, Ordering::Relaxed)
            )),
        };

        // Emit trace: tool call start
        self.trace.tool_call(name, &args);

        // Dispatch through registry (which does permission checks)
        let caps_snapshot = self.caps.snapshot();
        let result = self.registry.dispatch(&call, &caps_snapshot).await;

        // Emit trace: tool result
        self.trace
            .tool_result(name, &result.output, result.duration_us);

        if let Some(err) = &result.error {
            return Err(bua_core::BuaError::ToolCallFailed {
                name: name.to_string(),
                reason: err.clone(),
            });
        }

        Ok(result.output)
    }

    /// List available tools (for JS `Bua.tools.list()`).
    pub fn list(&self) -> Vec<serde_json::Value> {
        self.registry
            .list()
            .into_iter()
            .map(|(name, schema)| {
                serde_json::json!({
                    "name": name,
                    "description": schema.description,
                    "parameters": schema.parameters,
                })
            })
            .collect()
    }

    pub fn call_count(&self) -> u64 {
        self.call_count.load(Ordering::Relaxed)
    }
}
