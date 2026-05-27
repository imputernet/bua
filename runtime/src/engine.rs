/// engine.rs — JavaScriptCore embedding abstraction
///
/// The actual JSC C API calls are gated behind the `jsc` feature and the
/// C++ bridge in /jsc/. During MVP development this module exposes a clean
/// Rust API and the implementation can be swapped between a stub (for
/// unit-testing the Rust layers) and the real JSC binding.
use bua_core::{BuaError, BuaResult, CapabilitySet};
use parking_lot::Mutex;
use serde_json::Value;
use std::sync::Arc;

/// Configuration for a single JS engine instance.
#[derive(Debug, Clone)]
pub struct JsEngineConfig {
    /// Max heap size in bytes. None = JSC default.
    pub max_heap_bytes: Option<usize>,
    /// Enable JSC's built-in sampling profiler.
    pub enable_profiler: bool,
    /// Whether to expose `Bua.*` globals into the global scope.
    pub inject_bua_globals: bool,
    /// Snapshot bytecode to restore from, if any.
    pub snapshot_bytes: Option<Vec<u8>>,
}

impl Default for JsEngineConfig {
    fn default() -> Self {
        Self {
            max_heap_bytes: Some(256 * 1024 * 1024), // 256 MiB
            enable_profiler: false,
            inject_bua_globals: true,
            snapshot_bytes: None,
        }
    }
}

/// The result of evaluating a JS expression.
#[derive(Debug)]
pub struct EvalResult {
    pub value: JsValue,
    pub duration_us: u64,
}

/// A Rust-side representation of a JS value.
/// Intentionally minimal — complex objects are serialized via JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum JsValue {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    /// Arbitrary object/array serialized as JSON.
    Json(Value),
}

impl JsValue {
    pub fn into_json(self) -> Value {
        match self {
            JsValue::Undefined => Value::Null,
            JsValue::Null => Value::Null,
            JsValue::Bool(b) => Value::Bool(b),
            JsValue::Number(n) => serde_json::json!(n),
            JsValue::String(s) => Value::String(s),
            JsValue::Json(v) => v,
        }
    }
}

/// Callback type for Bua native functions exposed to JS.
pub type NativeFunction = Arc<dyn Fn(Vec<JsValue>) -> BuaResult<JsValue> + Send + Sync + 'static>;

/// A sandboxed JSC virtual machine.
///
/// Each `JsEngine` owns exactly one JSC context group and one global context.
/// Multiple engines may run concurrently in separate OS threads.
///
/// SAFETY: JSC contexts are NOT thread-safe by default. All JS execution
/// must be serialized through the inner Mutex.
pub struct JsEngine {
    config: JsEngineConfig,
    capabilities: Arc<Mutex<CapabilitySet>>,
    /// Inner state — behind a mutex so the engine can be shared via Arc.
    inner: Arc<Mutex<EngineInner>>,
}

struct EngineInner {
    /// Opaque pointer to JSC context. In real build: *mut JSGlobalContextRef.
    /// During stub mode: unused.
    #[allow(dead_code)]
    ctx_ptr: usize,
    registered_functions: Vec<String>,
}

impl JsEngine {
    /// Create a new engine with the given config and capability set.
    pub fn new(config: JsEngineConfig, capabilities: CapabilitySet) -> BuaResult<Self> {
        let inner = EngineInner {
            ctx_ptr: 0, // JSC_init() in real build
            registered_functions: Vec::new(),
        };

        let engine = Self {
            config,
            capabilities: Arc::new(Mutex::new(capabilities)),
            inner: Arc::new(Mutex::new(inner)),
        };

        if engine.config.inject_bua_globals {
            engine.inject_globals()?;
        }

        Ok(engine)
    }

    /// Register a native Rust function accessible as `Bua.native.<name>()` in JS.
    pub fn register_native(&self, name: &str, _func: NativeFunction) -> BuaResult<()> {
        let mut inner = self.inner.lock();
        inner.registered_functions.push(name.to_string());
        tracing::debug!(name, "registered native function");
        Ok(())
    }

    /// Evaluate a JS source string. Returns the completion value.
    ///
    /// In real build this calls JSEvaluateScript() after permission checks.
    pub fn eval(&self, source: &str, source_url: Option<&str>) -> BuaResult<EvalResult> {
        let start = std::time::Instant::now();
        tracing::debug!(source_url, bytes = source.len(), "eval");

        // Stub: parse as JSON if possible, else return undefined.
        let value = if source.trim_start().starts_with('{') || source.trim_start().starts_with('[')
        {
            serde_json::from_str::<Value>(source)
                .map(JsValue::Json)
                .unwrap_or(JsValue::Undefined)
        } else {
            JsValue::Undefined
        };

        Ok(EvalResult {
            value,
            duration_us: start.elapsed().as_micros() as u64,
        })
    }

    /// Evaluate a module entrypoint (ESM).
    pub async fn eval_module(&self, path: &std::path::Path) -> BuaResult<EvalResult> {
        let source = tokio::fs::read_to_string(path)
            .await
            .map_err(BuaError::Io)?;
        self.eval(&source, Some(&path.to_string_lossy()))
    }

    /// Update the capability set (e.g., after runtime revocation).
    pub fn set_capabilities(&self, caps: CapabilitySet) {
        *self.capabilities.lock() = caps;
    }

    /// Snapshot the current heap state to bytes for replay.
    pub fn snapshot(&self) -> BuaResult<Vec<u8>> {
        // In real build: JSC snapshot API.
        // Stub: empty snapshot.
        tracing::info!("heap snapshot captured (stub)");
        Ok(vec![0x0B, 0x0A, 0x0A]) // magic bytes
    }

    fn inject_globals(&self) -> BuaResult<()> {
        // In real build: inject Bua.* global object via JSC C API.
        tracing::debug!("Bua globals injected (stub)");
        Ok(())
    }
}

impl std::fmt::Debug for JsEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsEngine")
            .field("config", &self.config)
            .finish()
    }
}
