// runtime/src/runtime/runtime.rs
//
// Runtime is the top-level composed execution context for one agent.
//
// It owns all sub-contexts and enforces their lifecycle coordination:
//   - VmContext must be alive while ToolContext dispatches
//   - TraceContext receives events from all other contexts
//   - SnapshotContext can checkpoint any other context's state
//   - CapabilityContext gates all I/O in every other context

use bua_core::{AgentId, BuaResult, CapabilitySet, ExecutionId};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::agent_ctx::AgentLifecycle;
use super::snapshot_ctx::SnapshotConfig;
use super::vm::VmConfig;
use super::{
    AgentContext, CapabilityContext, SnapshotContext, ToolContext, TraceContext, VmContext,
};
use crate::tools::ToolRegistry;

/// Full configuration for a Runtime instance.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub entrypoint: PathBuf,
    pub capabilities: CapabilitySet,
    pub timeout: Option<Duration>,
    pub max_heap_bytes: usize,
    pub parent_id: Option<AgentId>,
    pub snapshot_config: Option<SnapshotConfig>,
    pub trace_enabled: bool,
}

impl RuntimeConfig {
    pub fn new(entrypoint: PathBuf, capabilities: CapabilitySet) -> Self {
        Self {
            entrypoint,
            capabilities,
            timeout: Some(Duration::from_secs(300)),
            max_heap_bytes: 256 * 1024 * 1024,
            parent_id: None,
            snapshot_config: None,
            trace_enabled: true,
        }
    }

    pub fn with_timeout(mut self, t: Duration) -> Self {
        self.timeout = Some(t);
        self
    }

    pub fn with_parent(mut self, id: AgentId) -> Self {
        self.parent_id = Some(id);
        self
    }

    pub fn with_snapshots(mut self, config: SnapshotConfig) -> Self {
        self.snapshot_config = Some(config);
        self
    }
}

/// The composed runtime for one agent execution.
///
/// After construction, call `Runtime::run()` to execute the entrypoint.
#[derive(Clone, Debug)]
pub struct Runtime {
    /// Agent identity and lifecycle state machine.
    pub agent: AgentContext,
    /// JS engine + module loader.
    pub vm: VmContext,
    /// Capability enforcement.
    pub caps: CapabilityContext,
    /// Tool dispatch with tracing.
    pub tools: ToolContext,
    /// Execution trace (all contexts write here).
    pub trace: TraceContext,
    /// Checkpoint/restore.
    pub snapshot: SnapshotContext,
}

impl Runtime {
    /// Construct a fully initialized Runtime for the given config.
    pub fn new(config: RuntimeConfig, tool_registry: Arc<ToolRegistry>) -> BuaResult<Self> {
        let agent = AgentContext::new(config.entrypoint.clone(), config.parent_id, config.timeout);

        let execution_id = agent.execution_id();

        let vm = VmContext::new(VmConfig {
            max_heap_bytes: config.max_heap_bytes,
            base_dir: config
                .entrypoint
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_path_buf(),
            drain_microtasks: true,
        })?;

        let caps = CapabilityContext::new(config.capabilities);

        let trace = if config.trace_enabled {
            TraceContext::new(execution_id.clone())
        } else {
            TraceContext::disabled()
        };

        let tools = ToolContext::new(tool_registry, caps.clone(), trace.clone());

        let snapshot =
            SnapshotContext::new(execution_id, config.snapshot_config.unwrap_or_default());

        Ok(Self {
            agent,
            vm,
            caps,
            tools,
            trace,
            snapshot,
        })
    }

    /// Execute the agent's entrypoint module.
    ///
    /// Handles lifecycle transitions, timeout enforcement, trace framing,
    /// and snapshot-on-completion.
    pub async fn run(&self) -> BuaResult<i32> {
        let entrypoint = self.agent.entrypoint();
        let agent_id = self.agent.id();

        self.trace.execution_start(&entrypoint.to_string_lossy());
        self.agent.transition_running();

        tracing::info!(%agent_id, entrypoint = %entrypoint.display(), "runtime starting");

        let result = if let Some(timeout) = self.agent.timeout() {
            tokio::time::timeout(timeout, self.vm.run_module(&entrypoint))
                .await
                .unwrap_or_else(|_| {
                    self.agent.transition_timed_out(timeout);
                    Err(bua_core::BuaError::AgentTimeout {
                        id: agent_id.to_string(),
                        timeout_ms: timeout.as_millis() as u64,
                    })
                })
        } else {
            self.vm.run_module(&entrypoint).await
        };

        let exit_code = match result {
            Ok(_) => {
                self.agent.transition_completed(0);
                self.trace.execution_end(0, self.elapsed_us());
                0
            }
            Err(ref e) => {
                let msg = e.to_string();
                if !self.agent.is_terminal() {
                    self.agent.transition_failed(msg.clone());
                }
                self.trace.js_exception(&msg, None);
                self.trace.execution_end(1, self.elapsed_us());
                1
            }
        };

        tracing::info!(
            %agent_id,
            exit_code,
            trace_events = self.trace.event_count(),
            "runtime finished"
        );

        result.map(|_| exit_code)
    }

    /// Spawn a child Runtime with a derived (restricted) capability set.
    pub fn spawn_child(
        &self,
        entrypoint: PathBuf,
        child_caps: CapabilitySet,
        tool_registry: Arc<ToolRegistry>,
    ) -> BuaResult<Runtime> {
        // Child capabilities MUST be a subset of parent's.
        let derived_caps = self.caps.derive_child(child_caps);
        let parent_id = self.agent.id();

        let config = RuntimeConfig {
            entrypoint,
            capabilities: derived_caps.snapshot(),
            timeout: self.agent.timeout(),
            max_heap_bytes: 128 * 1024 * 1024, // children get half parent heap
            parent_id: Some(parent_id),
            snapshot_config: None,
            trace_enabled: true,
        };

        Runtime::new(config, tool_registry)
    }

    // --- Accessors ---

    pub fn agent_id(&self) -> AgentId {
        self.agent.id()
    }

    pub fn execution_id(&self) -> ExecutionId {
        self.agent.execution_id()
    }

    pub fn lifecycle(&self) -> AgentLifecycle {
        self.agent.lifecycle()
    }

    fn elapsed_us(&self) -> u64 {
        self.agent
            .elapsed()
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::default_tool_registry;
    use tempfile::TempDir;

    fn make_runtime(entrypoint: PathBuf) -> Runtime {
        let caps = CapabilitySet::unrestricted();
        let config = RuntimeConfig::new(entrypoint, caps);
        let tools = Arc::new(default_tool_registry());
        Runtime::new(config, tools).unwrap()
    }

    #[tokio::test]
    async fn runtime_runs_nonexistent_file_fails() {
        let rt = make_runtime(PathBuf::from("/nonexistent/agent.ts"));
        let result = rt.run().await;
        // Should fail (file not found) and agent transitions to Failed
        assert!(result.is_err() || matches!(rt.lifecycle(), AgentLifecycle::Failed { .. }));
    }

    #[tokio::test]
    async fn runtime_lifecycle_transitions() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("test.js");
        std::fs::write(&script, "// empty").unwrap();

        let rt = make_runtime(script);
        assert!(matches!(rt.lifecycle(), AgentLifecycle::Pending));

        let _ = rt.run().await;
        assert!(rt.agent.is_terminal());
    }

    #[tokio::test]
    async fn child_inherits_restricted_caps() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("child.js");
        std::fs::write(&script, "// child").unwrap();

        let rt = make_runtime(dir.path().join("parent.js"));
        let tools = Arc::new(default_tool_registry());
        let child_caps = CapabilitySet::new(); // empty
        let child = rt.spawn_child(script, child_caps, tools).unwrap();

        // Child should have empty caps (parent was unrestricted but child requested empty)
        assert!(!child.caps.check(&bua_core::Permission::AgentSpawn));
    }
}
