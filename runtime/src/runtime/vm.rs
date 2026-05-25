// runtime/src/runtime/vm.rs
//
// VmContext owns the JS engine handle and module loader for one agent.
// It is the only entry point for executing JavaScript.

use bua_core::{BuaError, BuaResult};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ffi::engine::{JscEngine, JscEngineConfig};
use crate::ffi::value::JsValue;
use crate::loader::ModuleLoader;
use crate::transpiler::Transpiler;

#[derive(Debug, Clone)]
pub struct VmConfig {
    pub max_heap_bytes: usize,
    pub base_dir: PathBuf,
    pub drain_microtasks: bool,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            max_heap_bytes: 256 * 1024 * 1024,
            base_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            drain_microtasks: true,
        }
    }
}

/// Owns the JS engine and module loader for one agent.
#[derive(Clone)]
pub struct VmContext {
    pub(crate) engine: JscEngine,
    pub(crate) loader: Arc<ModuleLoader>,
}

impl VmContext {
    pub fn new(config: VmConfig) -> BuaResult<Self> {
        let engine = JscEngine::spawn(JscEngineConfig {
            max_heap_bytes: config.max_heap_bytes,
            drain_microtasks_after_eval: config.drain_microtasks,
        })?;

        let loader = Arc::new(ModuleLoader::new(
            config.base_dir,
            Transpiler::default(),
        ));

        Ok(Self { engine, loader })
    }

    /// Load, transpile, and execute a module entrypoint.
    pub async fn run_module(&self, entrypoint: &Path) -> BuaResult<JsValue> {
        let module = self.loader.load(
            &entrypoint.to_string_lossy(),
            None,
        ).await?;

        let source = module.source.to_string();
        let url = module.resolved_path.to_string_lossy().into_owned();

        self.engine.eval_module(source, url).await
    }

    /// Evaluate a raw JS/TS snippet (transpiles if needed).
    pub async fn eval_snippet(&self, source: &str, url: Option<&str>) -> BuaResult<JsValue> {
        // Transpile if it looks like TypeScript
        let js = if source.contains(": ") || source.contains("interface ") {
            let transpiler = Transpiler::default();
            transpiler.transpile(source, url.unwrap_or("<snippet>"))?.code
        } else {
            source.to_string()
        };

        self.engine.eval(js, url.map(str::to_string)).await
    }

    pub fn is_alive(&self) -> bool {
        self.engine.is_alive()
    }

    pub fn loader(&self) -> &ModuleLoader {
        &self.loader
    }
}

impl std::fmt::Debug for VmContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VmContext")
            .field("engine_alive", &self.engine.is_alive())
            .field("module_cache", &self.loader.cache_size())
            .finish()
    }
}
