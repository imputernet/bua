/// agent.rs — Agent lifecycle management
///
/// Each agent is an isolated JS execution context with its own:
///   - CapabilitySet (cannot exceed parent's)
///   - ToolRegistry view
///   - ExecutionTrace
///   - Tokio task handle
use bua_core::{AgentId, BuaError, BuaResult, CapabilitySet, ExecutionId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::engine::{JsEngine, JsEngineConfig};
use crate::tools::ToolRegistry;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub id: AgentId,
    pub entrypoint: PathBuf,
    pub capabilities: CapabilitySet,
    pub timeout: Option<Duration>,
    pub max_heap_bytes: Option<usize>,
    pub parent_id: Option<AgentId>,
    /// Environment variables passed to this agent.
    pub env: Vec<(String, String)>,
}

impl AgentConfig {
    pub fn new(entrypoint: PathBuf, capabilities: CapabilitySet) -> Self {
        Self {
            id: AgentId::new(),
            entrypoint,
            capabilities,
            timeout: Some(Duration::from_secs(300)),
            max_heap_bytes: Some(256 * 1024 * 1024),
            parent_id: None,
            env: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Messages sent into the agent's task
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum AgentMessage {
    /// Call a tool from within JS (bridged via native function).
    ToolCall {
        call: crate::tools::ToolCall,
        reply: oneshot::Sender<crate::tools::ToolResult>,
    },
    /// Graceful shutdown.
    Shutdown,
}

// ---------------------------------------------------------------------------
// Agent status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Pending,
    Running,
    Completed { exit_code: i32 },
    Failed { error: String },
    TimedOut,
}

// ---------------------------------------------------------------------------
// AgentHandle — the supervisor's view of a running agent
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct AgentHandle {
    pub id: AgentId,
    pub execution_id: ExecutionId,
    tx: mpsc::Sender<AgentMessage>,
    join: Option<JoinHandle<AgentStatus>>,
}

impl AgentHandle {
    pub async fn dispatch_tool(
        &self,
        call: crate::tools::ToolCall,
    ) -> BuaResult<crate::tools::ToolResult> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(AgentMessage::ToolCall {
                call,
                reply: reply_tx,
            })
            .await
            .map_err(|_| BuaError::internal("agent channel closed"))?;
        reply_rx
            .await
            .map_err(|_| BuaError::internal("agent reply channel closed"))
    }

    pub async fn shutdown(mut self) -> AgentStatus {
        let _ = self.tx.send(AgentMessage::Shutdown).await;
        if let Some(h) = self.join.take() {
            h.await.unwrap_or(AgentStatus::Failed {
                error: "join error".into(),
            })
        } else {
            AgentStatus::Completed { exit_code: 0 }
        }
    }
}

// ---------------------------------------------------------------------------
// Agent — spawns a task and returns a handle
// ---------------------------------------------------------------------------

pub struct Agent;

impl Agent {
    /// Spawn an agent task and return its handle.
    pub fn spawn(config: AgentConfig, tools: Arc<ToolRegistry>) -> BuaResult<AgentHandle> {
        let (tx, mut rx) = mpsc::channel::<AgentMessage>(64);
        let execution_id = ExecutionId::new();
        let agent_id = config.id.clone();
        let exec_id_clone = execution_id.clone();

        let task = tokio::spawn(async move {
            let engine_config = JsEngineConfig {
                max_heap_bytes: config.max_heap_bytes,
                inject_bua_globals: true,
                ..Default::default()
            };

            let engine = match JsEngine::new(engine_config, config.capabilities.clone()) {
                Ok(e) => e,
                Err(e) => {
                    return AgentStatus::Failed {
                        error: e.to_string(),
                    }
                }
            };

            tracing::info!(
                agent_id = %config.id,
                execution_id = %exec_id_clone,
                entrypoint = %config.entrypoint.display(),
                "agent started"
            );

            // Spawn the JS evaluation as a separate blocking task
            // (real build: runs JSC eval on a dedicated thread).
            let eval_handle = {
                let path = config.entrypoint.clone();
                tokio::spawn(async move { engine.eval_module(&path).await })
            };

            // Message pump — handles tool calls while JS runs.
            let mut eval_handle = eval_handle;
            let status = loop {
                tokio::select! {
                    msg = rx.recv() => {
                        match msg {
                            Some(AgentMessage::ToolCall { call, reply }) => {
                                let result = tools.dispatch(&call, &config.capabilities).await;
                                let _ = reply.send(result);
                            }
                            Some(AgentMessage::Shutdown) | None => {
                                break AgentStatus::Completed { exit_code: 0 };
                            }
                        }
                    }
                    result = &mut eval_handle => {
                        match result {
                            Ok(Ok(_)) => break AgentStatus::Completed { exit_code: 0 },
                            Ok(Err(e)) => break AgentStatus::Failed { error: e.to_string() },
                            Err(e) => break AgentStatus::Failed { error: e.to_string() },
                        }
                    }
                }
            };

            tracing::info!(agent_id = %config.id, ?status, "agent finished");
            status
        });

        // Apply timeout wrapper if configured
        let timeout = config.timeout;
        let final_task = if let Some(dur) = timeout {
            let id = agent_id.clone();
            tokio::spawn(async move {
                match tokio::time::timeout(dur, task).await {
                    Ok(Ok(s)) => s,
                    Ok(Err(_)) => AgentStatus::Failed {
                        error: "join error".into(),
                    },
                    Err(_) => {
                        tracing::warn!(agent_id = %id, "agent timed out");
                        AgentStatus::TimedOut
                    }
                }
            })
        } else {
            tokio::spawn(async move {
                task.await.unwrap_or(AgentStatus::Failed {
                    error: "join error".into(),
                })
            })
        };

        Ok(AgentHandle {
            id: agent_id,
            execution_id,
            tx,
            join: Some(final_task),
        })
    }
}
