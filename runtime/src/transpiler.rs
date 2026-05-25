/// transpiler.rs — TypeScript → JavaScript via SWC
///
/// Strips TypeScript types, preserves ESM syntax, handles JSX (tsx).
/// Intentionally zero config — Bua picks sensible defaults.

use bua_core::{BuaError, BuaResult};
use std::path::Path;
use std::sync::OnceLock;

/// Target JS version emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Es2022,
    Es2020,
}

/// Transpiler configuration.
#[derive(Debug, Clone)]
pub struct TranspilerConfig {
    pub target: Target,
    pub source_maps: bool,
    pub minify: bool,
}

impl Default for TranspilerConfig {
    fn default() -> Self {
        Self {
            target: Target::Es2022,
            source_maps: false,
            minify: false,
        }
    }
}

/// Output of a transpile operation.
#[derive(Debug)]
pub struct TranspileOutput {
    pub code: String,
    pub source_map: Option<String>,
    /// Time taken in microseconds.
    pub duration_us: u64,
}

/// Stateless transpiler — cheap to clone, holds no heap state.
#[derive(Debug, Clone, Default)]
pub struct Transpiler {
    pub config: TranspilerConfig,
}

impl Transpiler {
    pub fn new(config: TranspilerConfig) -> Self {
        Self { config }
    }

    /// Detect whether a file needs transpilation based on extension.
    pub fn needs_transpile(path: &Path) -> bool {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("ts") | Some("tsx") | Some("mts")
        )
    }

    /// Transpile TypeScript source to JavaScript.
    ///
    /// In the real build this calls into swc_core. This implementation
    /// performs a best-effort regex-free strip of TypeScript syntax
    /// sufficient for the stub runtime (no SWC dependency needed yet).
    pub fn transpile(
        &self,
        source: &str,
        filename: &str,
    ) -> BuaResult<TranspileOutput> {
        let start = std::time::Instant::now();

        // Real build uses swc_core:
        //   let cm = Arc::new(SourceMap::default());
        //   let compiler = Compiler::new(cm.clone());
        //   compiler.process_js_file(...)
        //
        // Stub: strip common TS annotations line-by-line.
        let js = strip_typescript_stub(source);

        let duration_us = start.elapsed().as_micros() as u64;
        tracing::debug!(filename, duration_us, "transpiled");

        Ok(TranspileOutput {
            code: js,
            source_map: None,
            duration_us,
        })
    }

    /// Transpile a file from disk.
    pub async fn transpile_file(&self, path: &Path) -> BuaResult<TranspileOutput> {
        let source = tokio::fs::read_to_string(path)
            .await
            .map_err(BuaError::Io)?;
        let filename = path.to_string_lossy().into_owned();
        self.transpile(&source, &filename)
    }
}

/// Best-effort TS → JS stub stripping type annotations.
/// NOT a full parser — used only until SWC is wired in.
fn strip_typescript_stub(src: &str) -> String {
    let mut out = String::with_capacity(src.len());

    for line in src.lines() {
        let trimmed = line.trim_start();

        // Drop import type lines
        if trimmed.starts_with("import type ") {
            out.push('\n');
            continue;
        }

        // Drop export type lines
        if trimmed.starts_with("export type ") || trimmed.starts_with("export interface ") {
            out.push('\n');
            continue;
        }

        // Drop interface declarations
        if trimmed.starts_with("interface ") {
            out.push('\n');
            continue;
        }

        // Drop type alias declarations
        if trimmed.starts_with("type ") && trimmed.contains(" = ") {
            out.push('\n');
            continue;
        }

        // Drop declare statements
        if trimmed.starts_with("declare ") {
            out.push('\n');
            continue;
        }

        out.push_str(line);
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_import_type() {
        let src = r#"
import type { Foo } from './foo';
import { bar } from './bar';
const x = bar();
"#;
        let t = Transpiler::default();
        let out = t.transpile(src, "test.ts").unwrap();
        assert!(!out.code.contains("import type"));
        assert!(out.code.contains("import { bar }"));
    }

    #[test]
    fn strips_interface() {
        let src = "interface Foo { x: number; }\nconst y = 1;\n";
        let t = Transpiler::default();
        let out = t.transpile(src, "test.ts").unwrap();
        assert!(!out.code.contains("interface"));
        assert!(out.code.contains("const y = 1;"));
    }
}
