/// loader.rs — ESM module loader
///
/// Resolves, loads, transpiles, and caches ES modules.
/// Resolution order: 1) exact path, 2) .ts/.js extension probing,
/// 3) node_modules (minimal, for stdlib-like packages), 4) error.

use bua_core::{BuaError, BuaResult};
use dashmap::DashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::transpiler::{Transpiler, TranspileOutput};

/// A loaded, ready-to-evaluate module.
#[derive(Debug, Clone)]
pub struct Module {
    pub specifier: String,
    pub resolved_path: PathBuf,
    pub source: Arc<str>,
    /// True if the source was transpiled from TypeScript.
    pub was_transpiled: bool,
}

/// Cache entry.
#[derive(Clone)]
struct CacheEntry {
    module: Arc<Module>,
    /// File mtime when cached. Used for cache invalidation.
    mtime: std::time::SystemTime,
}

/// ESM module loader with in-memory LRU cache.
pub struct ModuleLoader {
    transpiler: Transpiler,
    cache: DashMap<PathBuf, CacheEntry>,
    /// Root directory for relative resolution.
    base_dir: PathBuf,
}

impl ModuleLoader {
    pub fn new(base_dir: PathBuf, transpiler: Transpiler) -> Self {
        Self {
            transpiler,
            cache: DashMap::new(),
            base_dir,
        }
    }

    /// Resolve a specifier to an absolute path.
    pub fn resolve(&self, specifier: &str, referrer: Option<&Path>) -> BuaResult<PathBuf> {
        // Absolute path
        if specifier.starts_with('/') {
            return probe_extensions(Path::new(specifier));
        }

        // Relative path
        if specifier.starts_with("./") || specifier.starts_with("../") {
            let base = referrer
                .and_then(|p| p.parent())
                .unwrap_or(&self.base_dir);
            let candidate = base.join(specifier);
            return probe_extensions(&candidate);
        }

        // Bua built-ins (bua:*)
        if let Some(builtin) = specifier.strip_prefix("bua:") {
            return Ok(PathBuf::from(format!("__bua_builtin__/{builtin}")));
        }

        Err(BuaError::ModuleNotFound {
            specifier: specifier.to_string(),
        })
    }

    /// Load a module by specifier, using the cache when fresh.
    pub async fn load(&self, specifier: &str, referrer: Option<&Path>) -> BuaResult<Arc<Module>> {
        let path = self.resolve(specifier, referrer)?;

        // Check cache freshness
        if let Some(entry) = self.cache.get(&path) {
            if let Ok(meta) = tokio::fs::metadata(&path).await {
                if let Ok(mtime) = meta.modified() {
                    if mtime <= entry.mtime {
                        tracing::trace!(specifier, "module cache hit");
                        return Ok(entry.module.clone());
                    }
                }
            }
        }

        // Load from disk
        let raw = tokio::fs::read_to_string(&path).await.map_err(|e| {
            BuaError::ModuleLoadFailed {
                specifier: specifier.to_string(),
                reason: e.to_string(),
            }
        })?;

        let (source, was_transpiled) = if Transpiler::needs_transpile(&path) {
            let out = self.transpiler.transpile(&raw, &path.to_string_lossy())?;
            (out.code, true)
        } else {
            (raw, false)
        };

        let module = Arc::new(Module {
            specifier: specifier.to_string(),
            resolved_path: path.clone(),
            source: Arc::from(source.as_str()),
            was_transpiled,
        });

        let mtime = tokio::fs::metadata(&path)
            .await
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::SystemTime::now());

        self.cache.insert(
            path,
            CacheEntry {
                module: module.clone(),
                mtime,
            },
        );

        tracing::debug!(specifier, was_transpiled, "module loaded");
        Ok(module)
    }

    /// Invalidate the entire module cache (e.g., on file watch event).
    pub fn invalidate_all(&self) {
        self.cache.clear();
        tracing::debug!("module cache cleared");
    }

    /// Return the number of cached modules.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }
}

/// Probe for a path with various extensions.
fn probe_extensions(base: &Path) -> BuaResult<PathBuf> {
    // Exact path first
    if base.exists() && base.is_file() {
        return Ok(base.to_path_buf());
    }

    // Extension probing
    let extensions = ["", ".ts", ".js", ".mts", ".mjs", ".tsx", ".jsx"];
    for ext in &extensions {
        let candidate = if ext.is_empty() {
            base.to_path_buf()
        } else {
            let mut p = base.to_path_buf();
            let name = base
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            p.set_file_name(format!("{name}{ext}"));
            p
        };

        if candidate.exists() && candidate.is_file() {
            return Ok(candidate);
        }

        // index file inside directory
        if candidate.is_dir() {
            for idx_ext in &[".ts", ".js", ".mts", ".mjs"] {
                let idx = candidate.join(format!("index{idx_ext}"));
                if idx.exists() {
                    return Ok(idx);
                }
            }
        }
    }

    Err(BuaError::ModuleNotFound {
        specifier: base.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[tokio::test]
    async fn loads_js_module() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("hello.js");
        std::fs::write(&file, "export const x = 1;").unwrap();

        let loader = ModuleLoader::new(dir.path().to_path_buf(), Transpiler::default());
        let m = loader.load("./hello.js", Some(&file)).await.unwrap();
        assert!(m.source.contains("export const x = 1;"));
        assert!(!m.was_transpiled);
    }

    #[tokio::test]
    async fn loads_and_transpiles_ts_module() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("hello.ts");
        std::fs::write(&file, "type Foo = string;\nexport const x: Foo = 'hi';").unwrap();

        let loader = ModuleLoader::new(dir.path().to_path_buf(), Transpiler::default());
        let m = loader.load("./hello.ts", Some(&file)).await.unwrap();
        assert!(m.was_transpiled);
        assert!(!m.source.contains("type Foo"));
    }
}
