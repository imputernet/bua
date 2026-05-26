// runtime/src/promise/bridge.rs
//
// PromiseBridge is the coordinator between:
//   - JS code that creates Promises and awaits them
//   - Rust async tasks that produce values
//   - The ResolutionQueue that safely delivers results back to JS
//
// Lifecycle of a bridged async call:
//
//   1. JS calls Bua.tools.call("name", args)
//   2. Native callback fires on JS thread
//   3. JscContext::create_promise() -> (PromiseHandle, promise_val)
//   4. promise_val returned to JS (JS can await it)
//   5. PromiseBridge::spawn_task(handle, future) called
//   6. Tokio spawns the async task
//   7. Task completes -> ResolutionQueue::push(Resolution::resolve/reject)
//   8. After current JS eval returns, drain() is called
//   9. JscContext::resolve_promise(handle, value) called
//  10. JSC calls the stored resolve fn -> Promise resolves
//  11. drain_microtasks() -> JS continuations run

use bua_core::{BuaError, BuaResult};
use dashmap::DashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;

use super::queue::{Resolution, ResolutionQueue};
use crate::ffi::value::{JsException, JsValue, PromiseHandle};

/// Unique identifier for a tracked promise.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PromiseId(Uuid);

impl PromiseId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for PromiseId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for PromiseId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "prom-{}", self.0.as_simple())
    }
}

/// A live pending promise.
#[derive(Debug)]
#[allow(dead_code)]
struct PendingPromise {
    id: PromiseId,
    handle: PromiseHandle,
    created_us: u64,
}

fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

/// Coordinates async Rust tasks with JS Promises.
///
/// Thread-safe: can be shared between the JS thread and Tokio threads.
#[derive(Clone, Debug)]
pub struct PromiseBridge {
    /// Live promises awaiting resolution.
    pending: Arc<DashMap<PromiseId, PendingPromise>>,
    /// Queue where Tokio tasks post results.
    resolution_queue: ResolutionQueue,
    /// Monotonic promise counter.
    promise_count: Arc<AtomicU64>,
}

impl PromiseBridge {
    pub fn new(resolution_queue: ResolutionQueue) -> Self {
        Self {
            pending: Arc::new(DashMap::new()),
            resolution_queue,
            promise_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Register a promise handle and spawn a Tokio task for it.
    ///
    /// The future must produce a JsValue on success or JsException on failure.
    /// When it completes, the result is pushed to the ResolutionQueue.
    ///
    /// Returns the PromiseId for tracking/cancellation.
    pub fn spawn_task<F>(&self, handle: PromiseHandle, future: F) -> PromiseId
    where
        F: Future<Output = Result<JsValue, JsException>> + Send + 'static,
    {
        let id = PromiseId::new();
        self.promise_count.fetch_add(1, Ordering::Relaxed);

        let pending = PendingPromise {
            id: id.clone(),
            handle: handle.clone(),
            created_us: now_us(),
        };
        self.pending.insert(id.clone(), pending);

        let queue = self.resolution_queue.clone();
        let pending_map = self.pending.clone();
        let task_id = id.clone();

        tokio::spawn(async move {
            tracing::debug!(promise_id = %task_id, "promise task started");
            let result = future.await;

            let resolution = match result {
                Ok(value) => {
                    tracing::debug!(promise_id = %task_id, "promise resolved");
                    Resolution::resolve(handle, value)
                }
                Err(ex) => {
                    tracing::warn!(
                        promise_id = %task_id,
                        error = %ex.message,
                        "promise rejected"
                    );
                    Resolution::reject(handle, ex)
                }
            };

            queue.push(resolution);
            pending_map.remove(&task_id);
        });

        id
    }

    /// Spawn a Tokio task for a tool call, bridging it to a JS Promise.
    ///
    /// This is the primary entry point for the native `Bua.tools.call` callback.
    pub fn spawn_tool_call(
        &self,
        handle: PromiseHandle,
        _tool_name: String,
        tool_future: impl Future<Output = BuaResult<serde_json::Value>> + Send + 'static,
    ) -> PromiseId {
        self.spawn_task(handle, async move {
            match tool_future.await {
                Ok(json_val) => Ok(JsValue::from_json(json_val)),
                Err(e) => Err(JsException::new(e.to_string()).with_name(classify_error_name(&e))),
            }
        })
    }

    /// Drain the resolution queue.
    ///
    /// MUST be called from the JS thread at safe points.
    /// Returns resolutions ready to be delivered to JSC.
    pub fn drain_resolutions(&self) -> Vec<Resolution> {
        self.resolution_queue.drain()
    }

    /// Number of promises still pending.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Total promises ever created via this bridge.
    pub fn total_created(&self) -> u64 {
        self.promise_count.load(Ordering::Relaxed)
    }

    /// True if there are resolutions ready to deliver.
    pub fn has_ready_resolutions(&self) -> bool {
        self.resolution_queue.has_pending()
    }

    /// Check if any promises have been pending longer than `threshold_us` microseconds.
    /// Useful for timeout detection.
    pub fn stale_promises(&self, threshold_us: u64) -> Vec<PromiseId> {
        let now = now_us();
        self.pending
            .iter()
            .filter(|entry| now.saturating_sub(entry.value().created_us) > threshold_us)
            .map(|entry| entry.key().clone())
            .collect()
    }
}

/// Map BuaError variants to JS Error names for better JS-side error handling.
fn classify_error_name(e: &BuaError) -> &'static str {
    match e {
        BuaError::PermissionDenied { .. } => "PermissionError",
        BuaError::ModuleNotFound { .. } => "ModuleNotFoundError",
        BuaError::ToolNotFound { .. } => "ToolNotFoundError",
        BuaError::AgentTimeout { .. } => "TimeoutError",
        BuaError::Io(_) => "IOError",
        _ => "BuaError",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Duration;

    #[tokio::test]
    async fn spawn_and_resolve() {
        let queue = ResolutionQueue::new();
        let bridge = PromiseBridge::new(queue.clone());

        let handle = PromiseHandle::stub();
        let _id = bridge.spawn_task(handle, async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok(JsValue::Number(99.0))
        });

        assert_eq!(bridge.pending_count(), 1);
        assert_eq!(bridge.total_created(), 1);

        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(bridge.pending_count(), 0);
        assert_eq!(queue.pending_count(), 1);

        let drained = bridge.drain_resolutions();
        assert_eq!(drained.len(), 1);
        assert!(drained[0].is_resolve());
    }

    #[tokio::test]
    async fn spawn_and_reject() {
        let queue = ResolutionQueue::new();
        let bridge = PromiseBridge::new(queue.clone());

        bridge.spawn_task(PromiseHandle::stub(), async {
            Err(JsException::new("async failure"))
        });

        tokio::time::sleep(Duration::from_millis(20)).await;

        let drained = bridge.drain_resolutions();
        assert_eq!(drained.len(), 1);
        assert!(!drained[0].is_resolve());
    }

    #[tokio::test]
    async fn stale_promise_detection() {
        let queue = ResolutionQueue::new();
        let bridge = PromiseBridge::new(queue);

        // Spawn a task that never completes
        bridge.spawn_task(PromiseHandle::stub(), async {
            tokio::time::sleep(Duration::from_secs(999)).await;
            Ok(JsValue::Undefined)
        });

        tokio::time::sleep(Duration::from_millis(10)).await;

        // With a 0µs threshold, every pending promise is "stale"
        let stale = bridge.stale_promises(0);
        assert_eq!(stale.len(), 1);
    }
}
