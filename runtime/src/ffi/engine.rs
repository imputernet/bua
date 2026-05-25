// runtime/src/ffi/engine.rs
//
// JscEngine bridges async Tokio callers to the single-threaded JscContext.
//
// Architecture:
//   - JscContext lives on a dedicated OS thread (via std::thread::spawn)
//   - All JS operations are sent as closures over a oneshot channel
//   - The engine thread drains microtasks after every operation
//   - JscEngine itself is Send + Sync (safe to share via Arc)
//
// This is the pattern used by Node.js (libuv + V8 isolate thread) and Bun.
// It avoids JSC's lack of thread-safety while keeping the Tokio executor free.

use bua_core::{BuaError, BuaResult};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

use super::context::{JscContext, NativeFn};
use super::value::{JsException, JsValue, PromiseHandle};

// ---------------------------------------------------------------------------
// Work item sent to the JS thread
// ---------------------------------------------------------------------------

type JsResult = Result<JsValue, JsException>;
type JsWork = Box<dyn FnOnce(&mut JscContext) -> JsResult + Send + 'static>;

struct WorkItem {
    work: JsWork,
    reply: oneshot::Sender<JsResult>,
}

// ---------------------------------------------------------------------------
// JscEngine config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct JscEngineConfig {
    pub max_heap_bytes: usize,
    pub drain_microtasks_after_eval: bool,
}

impl Default for JscEngineConfig {
    fn default() -> Self {
        Self {
            max_heap_bytes: 256 * 1024 * 1024,
            drain_microtasks_after_eval: true,
        }
    }
}

// ---------------------------------------------------------------------------
// JscEngine — the public API, safe to clone and share
// ---------------------------------------------------------------------------

/// Thread-safe handle to the JS engine.
///
/// Internally the JS context runs on a dedicated OS thread.
/// Operations are dispatched via a channel and awaited asynchronously.
#[derive(Clone, Debug)]
pub struct JscEngine {
    tx: mpsc::Sender<WorkItem>,
    config: Arc<JscEngineConfig>,
}

impl JscEngine {
    /// Spin up the JS thread and return a handle.
    pub fn spawn(config: JscEngineConfig) -> BuaResult<Self> {
        let (tx, mut rx) = mpsc::channel::<WorkItem>(256);
        let max_heap = config.max_heap_bytes;
        let drain_after = config.drain_microtasks_after_eval;

        std::thread::Builder::new()
            .name("bua-js".into())
            .spawn(move || {
                // This thread owns JscContext for its entire lifetime.
                let mut ctx = match JscContext::new(max_heap) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("JscContext init failed: {e}");
                        return;
                    }
                };

                tracing::debug!("JS thread started");

                // Block on the channel — each item is a closure to run.
                while let Some(item) = rx.blocking_recv() {
                    let result = (item.work)(&mut ctx);

                    if drain_after {
                        ctx.drain_microtasks();
                    }

                    // Ignore send error — caller may have dropped the receiver.
                    let _ = item.reply.send(result);
                }

                tracing::debug!("JS thread exiting");
            })
            .map_err(|e| BuaError::JsEngineInit(e.to_string()))?;

        Ok(Self {
            tx,
            config: Arc::new(config),
        })
    }

    /// Evaluate a JS source string on the JS thread.
    pub async fn eval(&self, source: String, url: Option<String>) -> BuaResult<JsValue> {
        self.dispatch(move |ctx| ctx.eval(&source, url.as_deref()))
            .await
    }

    /// Evaluate a module source on the JS thread.
    pub async fn eval_module(&self, source: String, module_url: String) -> BuaResult<JsValue> {
        self.dispatch(move |ctx| ctx.eval_module(&source, &module_url))
            .await
    }

    /// Register a native Rust function callable from JS.
    pub async fn register_native(&self, path: String, f: NativeFn) -> BuaResult<()> {
        self.dispatch(move |ctx| {
            ctx.register_native(&path, f)
                .map(|_| JsValue::Undefined)
                .map_err(|e| JsException::new(e.to_string()))
        })
        .await
        .map(|_| ())
    }

    /// Call a function handle with arguments on the JS thread.
    pub async fn call_function(
        &self,
        func: super::value::FunctionHandle,
        args: Vec<JsValue>,
    ) -> BuaResult<JsValue> {
        self.dispatch(move |ctx| ctx.call_function(&func, None, args))
            .await
    }

    /// Resolve a JS Promise from async Rust.
    pub async fn resolve_promise(&self, handle: PromiseHandle, value: JsValue) -> BuaResult<()> {
        self.dispatch(move |ctx| {
            ctx.resolve_promise(&handle, value)
                .map(|_| JsValue::Undefined)
                .map_err(|e| JsException::new(e.to_string()))
        })
        .await
        .map(|_| ())
    }

    /// Reject a JS Promise from async Rust.
    pub async fn reject_promise(&self, handle: PromiseHandle, ex: JsException) -> BuaResult<()> {
        self.dispatch(move |ctx| {
            ctx.reject_promise(&handle, ex)
                .map(|_| JsValue::Undefined)
                .map_err(|e| JsException::new(e.to_string()))
        })
        .await
        .map(|_| ())
    }

    /// Capture a heap snapshot.
    pub async fn snapshot_heap(&self) -> BuaResult<Vec<u8>> {
        self.dispatch(|ctx| {
            ctx.snapshot_heap()
                .map(|bytes| JsValue::String(hex::encode(&bytes))) // carry bytes as hex; real build returns raw
                .map_err(|e| JsException::new(e.to_string()))
        })
        .await
        .and_then(|v| {
            // Decode hex back to bytes
            if let JsValue::String(hex_str) = v {
                hex::decode(&hex_str).map_err(|e| BuaError::internal(e.to_string()))
            } else {
                Ok(vec![])
            }
        })
    }

    /// Send arbitrary work to the JS thread and await the result.
    ///
    /// This is the escape hatch for operations not covered by the typed API.
    /// Keep callers minimal — prefer typed methods above.
    pub async fn dispatch<F>(&self, f: F) -> BuaResult<JsValue>
    where
        F: FnOnce(&mut JscContext) -> Result<JsValue, JsException> + Send + 'static,
    {
        let (reply_tx, reply_rx) = oneshot::channel();

        self.tx
            .send(WorkItem {
                work: Box::new(f),
                reply: reply_tx,
            })
            .await
            .map_err(|_| BuaError::internal("JS thread channel closed"))?;

        reply_rx
            .await
            .map_err(|_| BuaError::internal("JS thread reply dropped"))?
            .map_err(BuaError::from)
    }

    /// Check if the JS thread is still alive.
    pub fn is_alive(&self) -> bool {
        !self.tx.is_closed()
    }
}

// ---------------------------------------------------------------------------
// Hex helpers (no dep needed — minimal impl)
// ---------------------------------------------------------------------------

mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn decode(s: &str) -> Result<Vec<u8>, String> {
        if s.len() % 2 != 0 {
            return Err("odd length hex string".into());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn engine_spawns_and_evals() {
        let engine = JscEngine::spawn(JscEngineConfig::default()).unwrap();
        assert!(engine.is_alive());

        let result = engine
            .eval("42 + 1".into(), Some("test.js".into()))
            .await
            .unwrap();

        // Stub returns Undefined; real JSC returns Number(43).
        // Test the channel plumbing, not the JS result.
        drop(result);
    }

    #[tokio::test]
    async fn engine_channel_closed_after_drop() {
        let engine = JscEngine::spawn(JscEngineConfig::default()).unwrap();
        let tx = engine.tx.clone();
        drop(engine);
        // Allow thread to drain
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(tx.is_closed());
    }
}
