// runtime/src/promise/queue.rs
//
// ResolutionQueue is the boundary between Tokio async work and JSC.
//
// Rule: Tokio tasks ENQUEUE results here.
//       JSC drains the queue at safe points (after eval, between microtask turns).
//
// This prevents reentrancy: we never call into JSC while it's running.

use crate::ffi::value::{JsException, JsValue, PromiseHandle};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// A single promise resolution — either a value or a rejection.
#[derive(Debug)]
pub enum Resolution {
    Resolve {
        handle: PromiseHandle,
        value: JsValue,
    },
    Reject {
        handle: PromiseHandle,
        exception: JsException,
    },
}

impl Resolution {
    pub fn resolve(handle: PromiseHandle, value: JsValue) -> Self {
        Self::Resolve { handle, value }
    }

    pub fn reject(handle: PromiseHandle, exception: JsException) -> Self {
        Self::Reject { handle, exception }
    }

    pub fn is_resolve(&self) -> bool {
        matches!(self, Self::Resolve { .. })
    }
}

/// A thread-safe queue of pending promise resolutions.
///
/// Tokio tasks push to this queue.
/// The JS thread drains it at safe points between eval turns.
#[derive(Clone, Debug)]
pub struct ResolutionQueue {
    inner: Arc<QueueInner>,
}

#[derive(Debug)]
struct QueueInner {
    queue: Mutex<VecDeque<Resolution>>,
    /// Total resolutions enqueued (monotonic).
    total_enqueued: AtomicU64,
    /// Total resolutions drained.
    total_drained: AtomicU64,
}

impl ResolutionQueue {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(QueueInner {
                queue: Mutex::new(VecDeque::new()),
                total_enqueued: AtomicU64::new(0),
                total_drained: AtomicU64::new(0),
            }),
        }
    }

    /// Push a resolution. Called from Tokio threads. Lock-contended but rare.
    pub fn push(&self, resolution: Resolution) {
        self.inner.queue.lock().push_back(resolution);
        self.inner.total_enqueued.fetch_add(1, Ordering::Relaxed);
        tracing::trace!(
            pending = self.pending_count(),
            "resolution enqueued"
        );
    }

    /// Drain all pending resolutions. Called from the JS thread only.
    ///
    /// Returns the number of resolutions drained.
    pub fn drain(&self) -> Vec<Resolution> {
        let mut queue = self.inner.queue.lock();
        let count = queue.len();
        if count == 0 {
            return vec![];
        }

        let drained: Vec<Resolution> = queue.drain(..).collect();
        self.inner
            .total_drained
            .fetch_add(count as u64, Ordering::Relaxed);

        tracing::trace!(count, "resolutions drained");
        drained
    }

    /// Number of unprocessed resolutions waiting.
    pub fn pending_count(&self) -> usize {
        self.inner.queue.lock().len()
    }

    pub fn total_enqueued(&self) -> u64 {
        self.inner.total_enqueued.load(Ordering::Relaxed)
    }

    pub fn total_drained(&self) -> u64 {
        self.inner.total_drained.load(Ordering::Relaxed)
    }

    /// True if there are resolutions waiting to be drained.
    pub fn has_pending(&self) -> bool {
        !self.inner.queue.lock().is_empty()
    }
}

impl Default for ResolutionQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_and_drain() {
        let q = ResolutionQueue::new();
        assert_eq!(q.pending_count(), 0);

        q.push(Resolution::resolve(
            PromiseHandle::stub(),
            JsValue::Number(42.0),
        ));
        q.push(Resolution::reject(
            PromiseHandle::stub(),
            JsException::new("oops"),
        ));

        assert_eq!(q.pending_count(), 2);
        assert_eq!(q.total_enqueued(), 2);

        let drained = q.drain();
        assert_eq!(drained.len(), 2);
        assert_eq!(q.pending_count(), 0);
        assert_eq!(q.total_drained(), 2);
        assert!(drained[0].is_resolve());
    }

    #[test]
    fn drain_empty_returns_empty() {
        let q = ResolutionQueue::new();
        assert!(q.drain().is_empty());
    }
}
