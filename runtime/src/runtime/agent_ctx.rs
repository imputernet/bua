// runtime/src/runtime/agent_ctx.rs
//
// AgentContext owns the agent's identity and lifecycle state.
// Separate from VmContext so identity/status can be queried
// even when the JS engine is not yet started or has crashed.

use bua_core::{AgentId, ExecutionId};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Agent lifecycle states — strict state machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentLifecycle {
    /// Created but not yet started.
    Pending,
    /// JS engine running.
    Running {
        started_at_us: u64,
    },
    /// Successfully completed.
    Completed {
        exit_code: i32,
        duration_us: u64,
    },
    /// Failed with an error.
    Failed {
        error: String,
        duration_us: u64,
    },
    /// Killed by timeout.
    TimedOut {
        timeout_ms: u64,
    },
    /// Cancelled by parent or scheduler.
    Cancelled,
}

impl AgentLifecycle {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed { .. }
                | Self::Failed { .. }
                | Self::TimedOut { .. }
                | Self::Cancelled
        )
    }

    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }

    pub fn exit_code(&self) -> Option<i32> {
        if let Self::Completed { exit_code, .. } = self {
            Some(*exit_code)
        } else {
            None
        }
    }
}

fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

/// Static identity + dynamic lifecycle for one agent.
#[derive(Debug, Clone)]
pub struct AgentContext {
    inner: Arc<RwLock<AgentContextInner>>,
}

#[derive(Debug)]
struct AgentContextInner {
    pub id: AgentId,
    pub execution_id: ExecutionId,
    pub parent_id: Option<AgentId>,
    pub entrypoint: PathBuf,
    pub timeout: Option<Duration>,
    pub lifecycle: AgentLifecycle,
    pub start_instant: Option<Instant>,
}

impl AgentContext {
    pub fn new(
        entrypoint: PathBuf,
        parent_id: Option<AgentId>,
        timeout: Option<Duration>,
    ) -> Self {
        Self {
            inner: Arc::new(RwLock::new(AgentContextInner {
                id: AgentId::new(),
                execution_id: ExecutionId::new(),
                parent_id,
                entrypoint,
                timeout,
                lifecycle: AgentLifecycle::Pending,
                start_instant: None,
            })),
        }
    }

    // --- Identity accessors (cheap, lock-free read) ---

    pub fn id(&self) -> AgentId {
        self.inner.read().id.clone()
    }

    pub fn execution_id(&self) -> ExecutionId {
        self.inner.read().execution_id.clone()
    }

    pub fn parent_id(&self) -> Option<AgentId> {
        self.inner.read().parent_id.clone()
    }

    pub fn entrypoint(&self) -> PathBuf {
        self.inner.read().entrypoint.clone()
    }

    pub fn timeout(&self) -> Option<Duration> {
        self.inner.read().timeout
    }

    // --- Lifecycle transitions ---

    pub fn transition_running(&self) {
        let mut inner = self.inner.write();
        assert!(
            matches!(inner.lifecycle, AgentLifecycle::Pending),
            "can only start from Pending, was {:?}",
            inner.lifecycle
        );
        inner.lifecycle = AgentLifecycle::Running { started_at_us: now_us() };
        inner.start_instant = Some(Instant::now());
        tracing::debug!(agent_id = %inner.id, "agent → Running");
    }

    pub fn transition_completed(&self, exit_code: i32) {
        let mut inner = self.inner.write();
        let duration_us = inner
            .start_instant
            .map(|t| t.elapsed().as_micros() as u64)
            .unwrap_or(0);
        inner.lifecycle = AgentLifecycle::Completed { exit_code, duration_us };
        tracing::debug!(agent_id = %inner.id, exit_code, duration_us, "agent → Completed");
    }

    pub fn transition_failed(&self, error: String) {
        let mut inner = self.inner.write();
        let duration_us = inner
            .start_instant
            .map(|t| t.elapsed().as_micros() as u64)
            .unwrap_or(0);
        tracing::warn!(agent_id = %inner.id, %error, "agent → Failed");
        inner.lifecycle = AgentLifecycle::Failed { error, duration_us };
    }

    pub fn transition_timed_out(&self, timeout: Duration) {
        let mut inner = self.inner.write();
        tracing::warn!(agent_id = %inner.id, timeout_ms = timeout.as_millis(), "agent → TimedOut");
        inner.lifecycle = AgentLifecycle::TimedOut {
            timeout_ms: timeout.as_millis() as u64,
        };
    }

    pub fn transition_cancelled(&self) {
        let mut inner = self.inner.write();
        tracing::info!(agent_id = %inner.id, "agent → Cancelled");
        inner.lifecycle = AgentLifecycle::Cancelled;
    }

    // --- Lifecycle queries ---

    pub fn lifecycle(&self) -> AgentLifecycle {
        self.inner.read().lifecycle.clone()
    }

    pub fn is_terminal(&self) -> bool {
        self.inner.read().lifecycle.is_terminal()
    }

    pub fn is_running(&self) -> bool {
        self.inner.read().lifecycle.is_running()
    }

    pub fn elapsed(&self) -> Option<Duration> {
        self.inner.read().start_instant.map(|t| t.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_ctx() -> AgentContext {
        AgentContext::new(PathBuf::from("test.ts"), None, None)
    }

    #[test]
    fn pending_to_running_to_completed() {
        let ctx = make_ctx();
        assert!(matches!(ctx.lifecycle(), AgentLifecycle::Pending));

        ctx.transition_running();
        assert!(ctx.is_running());

        ctx.transition_completed(0);
        assert!(ctx.is_terminal());
        assert_eq!(ctx.lifecycle().exit_code(), Some(0));
    }

    #[test]
    fn pending_to_failed() {
        let ctx = make_ctx();
        ctx.transition_running();
        ctx.transition_failed("oops".into());
        assert!(ctx.is_terminal());
        assert!(matches!(ctx.lifecycle(), AgentLifecycle::Failed { .. }));
    }

    #[test]
    #[should_panic]
    fn cannot_start_twice() {
        let ctx = make_ctx();
        ctx.transition_running();
        ctx.transition_running(); // panics
    }
}
