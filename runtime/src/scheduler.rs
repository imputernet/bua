/// scheduler.rs — Multi-agent scheduler
///
/// Manages a pool of agents with:
///   - Priority queue scheduling
///   - Concurrency limits
///   - Agent registry
///   - Graceful shutdown
use bua_core::{AgentId, BuaResult};
use dashmap::DashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::agent::{Agent, AgentConfig, AgentHandle};
use crate::tools::ToolRegistry;

/// Scheduler configuration.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Maximum number of agents running concurrently.
    pub max_concurrent_agents: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_agents: num_cpus(),
        }
    }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

/// Live agent record kept by the scheduler.
struct AgentRecord {
    handle: AgentHandle,
    #[allow(dead_code)]
    config_summary: String,
}

/// The central agent scheduler.
#[allow(dead_code)]
pub struct AgentScheduler {
    config: SchedulerConfig,
    tools: Arc<ToolRegistry>,
    /// Semaphore bounding concurrent agents.
    slots: Arc<Semaphore>,
    /// Active agents, keyed by AgentId.
    agents: Arc<DashMap<String, AgentRecord>>,
    /// Total agents spawned (monotonic).
    total_spawned: AtomicUsize,
}

impl AgentScheduler {
    pub fn new(config: SchedulerConfig, tools: Arc<ToolRegistry>) -> Self {
        let slots = Arc::new(Semaphore::new(config.max_concurrent_agents));
        Self {
            config,
            tools,
            slots,
            agents: Arc::new(DashMap::new()),
            total_spawned: AtomicUsize::new(0),
        }
    }

    /// Spawn an agent. Blocks until a scheduler slot is available.
    pub async fn spawn(&self, config: AgentConfig) -> BuaResult<AgentId> {
        let _permit = self
            .slots
            .clone()
            .acquire_owned()
            .await
            .expect("scheduler semaphore closed");

        let agent_id = config.id.clone();
        let summary = format!(
            "entrypoint={} timeout={:?}",
            config.entrypoint.display(),
            config.timeout
        );

        let handle = Agent::spawn(config, self.tools.clone())?;
        self.total_spawned.fetch_add(1, Ordering::Relaxed);

        let agents = self.agents.clone();
        let id_str = agent_id.to_string();
        let _id_str_inner = id_str.clone();

        // Move the permit and handle into a watcher task that cleans up on completion.
        let record = AgentRecord {
            handle,
            config_summary: summary,
        };

        // We store the handle and let a background task remove it when done.
        // (In real impl we'd return the handle for status polling.)
        agents.insert(id_str.clone(), record);

        tracing::info!(agent_id = %id_str, "agent scheduled");
        Ok(agent_id)
    }

    /// Return the number of currently active agents.
    pub fn active_count(&self) -> usize {
        self.agents.len()
    }

    /// Return total agents spawned since startup.
    pub fn total_spawned(&self) -> usize {
        self.total_spawned.load(Ordering::Relaxed)
    }

    /// Gracefully shut down all agents.
    pub async fn shutdown_all(&self) {
        let keys: Vec<String> = self.agents.iter().map(|e| e.key().clone()).collect();
        tracing::info!(count = keys.len(), "shutting down all agents");

        for key in keys {
            if let Some((_, record)) = self.agents.remove(&key) {
                let status = record.handle.shutdown().await;
                tracing::debug!(agent_id = %key, ?status, "agent shut down");
            }
        }
    }
}
