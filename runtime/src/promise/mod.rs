// runtime/src/promise/mod.rs
//
// The Promise bridge pipeline:
//
//   JS Promise
//       ↓  (native resolve/reject callbacks registered in JSC)
//   PromiseHandle (Rust side, holds FunctionHandles)
//       ↓  (sent over channel to Tokio world)
//   PendingPromise (tracked in PromiseBridge)
//       ↓  (Tokio task completes, sends result)
//   ResolutionQueue (enqueued, never re-enters JSC mid-eval)
//       ↓  (drained after eval, between microtask turns)
//   JscEngine::resolve_promise / reject_promise
//       ↓  (JSC calls resolve fn, microtasks drain)
//   JS continuation resumes
//
// The critical rule enforced here:
//   NEVER call back into JSC while an eval is in progress.
//   All resolutions are queued and drained at safe points.

pub mod bridge;
pub mod future;
pub mod queue;

pub use bridge::PromiseBridge;
pub use future::JsPromiseFuture;
pub use queue::{Resolution, ResolutionQueue};
