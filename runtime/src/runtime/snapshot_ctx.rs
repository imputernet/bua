// runtime/src/runtime/snapshot_ctx.rs
//
// SnapshotContext manages checkpoint/restore for a single agent.
//
// A checkpoint captures:
//   - The agent's capability set at that moment
//   - The full execution trace up to that point
//   - The JSC heap bytecode (via VmContext)
//   - Pending tool call state
//
// Restoring a checkpoint:
//   1. Create a fresh JscContext
//   2. Restore heap from bytecode
//   3. Replay tool calls with recorded responses (not live)
//   4. Resume execution from the checkpoint point

use bua_core::{BuaResult, ExecutionId};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::snapshot::Snapshot;
use super::capability_ctx::CapabilityContext;
use super::trace_ctx::TraceContext;

/// Configuration for snapshot behavior.
#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    /// Directory to write snapshots to.
    pub dir: PathBuf,
    /// Maximum number of snapshots to retain (LRU eviction).
    pub max_snapshots: usize,
    /// Automatically snapshot every N tool calls.
    pub auto_snapshot_every: Option<u64>,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            dir: PathBuf::from(".bua/snapshots"),
            max_snapshots: 10,
            auto_snapshot_every: None,
        }
    }
}

/// Manages snapshots for one agent execution.
#[derive(Clone, Debug)]
pub struct SnapshotContext {
    execution_id: ExecutionId,
    config: Arc<SnapshotConfig>,
    /// Number of snapshots taken this execution.
    snapshot_count: Arc<std::sync::atomic::AtomicU64>,
}

impl SnapshotContext {
    pub fn new(execution_id: ExecutionId, config: SnapshotConfig) -> Self {
        Self {
            execution_id,
            config: Arc::new(config),
            snapshot_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Take a snapshot of the current execution state.
    pub async fn checkpoint(
        &self,
        caps: &CapabilityContext,
        trace: &TraceContext,
        heap_bytes: Vec<u8>,
    ) -> BuaResult<SnapshotRef> {
        let count = self.snapshot_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let filename = format!(
            "{}-snap-{count:04}.bsnap",
            self.execution_id.as_uuid().as_simple()
        );
        let path = self.config.dir.join(&filename);

        // Ensure directory exists
        tokio::fs::create_dir_all(&self.config.dir).await.ok();

        let snap = Snapshot::new(
            self.execution_id.clone(),
            caps.snapshot(),
            trace.to_ndjson(),
            heap_bytes,
        );

        snap.write_to_file(&path).await?;

        tracing::info!(
            path = %path.display(),
            snap_count = count + 1,
            trace_events = trace.event_count(),
            "checkpoint written"
        );

        Ok(SnapshotRef {
            path,
            execution_id: self.execution_id.clone(),
            sequence: count,
        })
    }

    /// Load the most recent snapshot for this execution.
    pub async fn latest_snapshot(&self) -> BuaResult<Option<Snapshot>> {
        let mut entries = match tokio::fs::read_dir(&self.config.dir).await {
            Ok(e) => e,
            Err(_) => return Ok(None),
        };

        let prefix = self.execution_id.as_uuid().as_simple().to_string();
        let mut matching: Vec<PathBuf> = Vec::new();

        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&prefix) && name.ends_with(".bsnap") {
                matching.push(entry.path());
            }
        }

        if matching.is_empty() {
            return Ok(None);
        }

        // Sort by filename (sequence number embedded in name)
        matching.sort();
        let latest = matching.last().unwrap();

        Ok(Some(Snapshot::read_from_file(latest).await?))
    }

    /// Evict old snapshots beyond the max_snapshots limit.
    pub async fn evict_old(&self) -> BuaResult<usize> {
        let mut entries = match tokio::fs::read_dir(&self.config.dir).await {
            Ok(e) => e,
            Err(_) => return Ok(0),
        };

        let prefix = self.execution_id.as_uuid().as_simple().to_string();
        let mut snaps: Vec<PathBuf> = Vec::new();

        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&prefix) && name.ends_with(".bsnap") {
                snaps.push(entry.path());
            }
        }

        snaps.sort();
        let to_remove = snaps.len().saturating_sub(self.config.max_snapshots);

        for path in snaps.iter().take(to_remove) {
            if let Err(e) = tokio::fs::remove_file(path).await {
                tracing::warn!(path = %path.display(), error = %e, "failed to evict snapshot");
            }
        }

        Ok(to_remove)
    }

    pub fn should_auto_checkpoint(&self, tool_call_count: u64) -> bool {
        self.config
            .auto_snapshot_every
            .map(|n| n > 0 && tool_call_count % n == 0)
            .unwrap_or(false)
    }
}

/// A reference to a written snapshot file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRef {
    pub path: PathBuf,
    pub execution_id: ExecutionId,
    pub sequence: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bua_core::ExecutionId;
    use tempfile::TempDir;

    #[tokio::test]
    async fn checkpoint_and_load() {
        let dir = TempDir::new().unwrap();
        let exec_id = ExecutionId::new();
        let config = SnapshotConfig {
            dir: dir.path().to_path_buf(),
            max_snapshots: 5,
            auto_snapshot_every: Some(10),
        };

        let snap_ctx = SnapshotContext::new(exec_id.clone(), config);
        let caps = CapabilityContext::deny_all();
        let trace = TraceContext::new(exec_id);

        trace.execution_start("test.ts");

        let snap_ref = snap_ctx
            .checkpoint(&caps, &trace, vec![0xB, 0xA])
            .await
            .unwrap();

        assert!(snap_ref.path.exists());

        let loaded = snap_ctx.latest_snapshot().await.unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().heap_bytes, vec![0xB, 0xA]);
    }

    #[test]
    fn auto_checkpoint_trigger() {
        let ctx = SnapshotContext::new(
            ExecutionId::new(),
            SnapshotConfig { auto_snapshot_every: Some(5), ..Default::default() },
        );
        assert!(!ctx.should_auto_checkpoint(4));
        assert!(ctx.should_auto_checkpoint(5));
        assert!(!ctx.should_auto_checkpoint(6));
        assert!(ctx.should_auto_checkpoint(10));
    }
}
