// runtime/src/modules/record.rs

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Lifecycle of a module in the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleStatus {
    /// Specifier resolved but not yet loaded.
    Unloaded,
    /// Source loaded, not yet parsed for imports.
    Loaded,
    /// Imports extracted; currently resolving dependencies (cycle detection marker).
    Linking,
    /// All dependencies resolved; ready to evaluate.
    Linked,
    /// Evaluation started (for top-level await cycle detection).
    Evaluating,
    /// Evaluation complete.
    Evaluated,
    /// Failed at some stage.
    Failed(String),
}

impl ModuleStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Evaluated | Self::Failed(_))
    }
    pub fn is_evaluatable(&self) -> bool {
        matches!(self, Self::Linked)
    }
    pub fn failed_reason(&self) -> Option<&str> {
        if let Self::Failed(r) = self {
            Some(r)
        } else {
            None
        }
    }
}

/// A single module node in the dependency graph.
#[derive(Debug, Clone)]
pub struct ModuleRecord {
    /// Canonical resolved path (or `bua:name` for builtins).
    pub resolved_path: PathBuf,
    /// Original specifier as written in source.
    pub specifier: String,
    /// Transformed JS source (post-TypeScript strip).
    pub source: String,
    /// Static import specifiers extracted from source.
    pub imports: Vec<ImportDecl>,
    /// Current lifecycle status.
    pub status: ModuleStatus,
    /// Source map JSON if generated during transform.
    pub source_map: Option<String>,
    /// Whether this module uses top-level await.
    pub has_top_level_await: bool,
    /// File modification time (nanoseconds) for cache invalidation.
    pub mtime_ns: Option<u128>,
}

/// A static import declaration extracted from module source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportDecl {
    /// The specifier as written: `"./foo"`, `"bua:fs"`, etc.
    pub specifier: String,
    /// True if this is a dynamic `import()` call (not statically analyzable).
    pub is_dynamic: bool,
    /// True if `import type` (TypeScript type-only, no runtime effect).
    pub is_type_only: bool,
}

impl ModuleRecord {
    pub fn new(resolved_path: PathBuf, specifier: String, source: String) -> Self {
        let imports = extract_static_imports(&source);
        let has_tla = detect_top_level_await(&source);

        Self {
            resolved_path,
            specifier,
            source,
            imports,
            status: ModuleStatus::Loaded,
            source_map: None,
            has_top_level_await: has_tla,
            mtime_ns: None,
        }
    }

    pub fn is_builtin(&self) -> bool {
        self.resolved_path
            .to_string_lossy()
            .starts_with("__bua_builtin__")
    }

    /// Mark this module as currently being linked (cycle detection entry point).
    pub fn begin_link(&mut self) {
        self.status = ModuleStatus::Linking;
    }

    /// Mark linking complete.
    pub fn finish_link(&mut self) {
        self.status = ModuleStatus::Linked;
    }

    /// Mark evaluation started.
    pub fn begin_eval(&mut self) {
        self.status = ModuleStatus::Evaluating;
    }

    /// Mark evaluation complete.
    pub fn finish_eval(&mut self) {
        self.status = ModuleStatus::Evaluated;
    }

    pub fn fail(&mut self, reason: String) {
        self.status = ModuleStatus::Failed(reason);
    }
}

// ---------------------------------------------------------------------------
// Static import extraction
// ---------------------------------------------------------------------------

/// Extract static import specifiers from JS/TS source.
///
/// Handles:
///   import foo from './foo'
///   import { bar } from './bar'
///   import type { Baz } from './baz'  (type-only)
///   export { x } from './x'
///
/// Does NOT handle dynamic `import()` — those are tracked separately at runtime.
///
/// This is a line-oriented heuristic, NOT a full parser.
/// Full accuracy requires swc_ecma_parser (Phase 2).
pub fn extract_static_imports(source: &str) -> Vec<ImportDecl> {
    let mut imports = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim();

        // Skip comments
        if trimmed.starts_with("//") || trimmed.starts_with("*") {
            continue;
        }

        // import ... from '...'
        // import type ... from '...'
        if trimmed.starts_with("import ") || trimmed.starts_with("export ") {
            let is_type_only = trimmed.contains(" type ") && !trimmed.contains(" type {");
            let is_type_only = is_type_only || trimmed.starts_with("import type ");

            if let Some(spec) = extract_quoted_specifier(trimmed) {
                // Skip bare side-effect imports with no specifier extraction
                if !spec.is_empty() {
                    imports.push(ImportDecl {
                        specifier: spec,
                        is_dynamic: false,
                        is_type_only,
                    });
                }
            }
        }

        // Dynamic import() detection — logged but not statically resolved
        if trimmed.contains("import(") {
            imports.push(ImportDecl {
                specifier: "<dynamic>".into(),
                is_dynamic: true,
                is_type_only: false,
            });
        }
    }

    imports
}

/// Extract the quoted specifier from an import/export line.
fn extract_quoted_specifier(line: &str) -> Option<String> {
    // Find last quoted string — that's the module specifier
    for quote in &['"', '\'', '`'] {
        if let Some(end) = line.rfind(*quote) {
            let before = &line[..end];
            if let Some(start) = before.rfind(*quote) {
                let spec = &line[start + 1..end];
                if !spec.is_empty()
                    && (spec.starts_with('.')
                        || spec.starts_with('/')
                        || spec.starts_with("bua:")
                        || !spec.contains(' '))
                {
                    return Some(spec.to_string());
                }
            }
        }
    }
    None
}

/// Detect top-level await in module source (heuristic).
pub fn detect_top_level_await(source: &str) -> bool {
    for line in source.lines() {
        let t = line.trim();
        if t.starts_with("//") {
            continue;
        }
        // Top-level await: `await expr` not inside a function body
        // Heuristic: `await ` at start of statement at top indentation
        if (t.starts_with("await ") || t.starts_with("const x = await") || t.contains("= await "))
            && !line.starts_with("  ")
            && !line.starts_with('\t')
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_import_specifiers() {
        let src = r#"
import foo from './foo.js';
import { bar } from '../bar';
import type { Baz } from './types';
import 'bua:fs';
export { x } from './x';
"#;
        let imports = extract_static_imports(src);
        let specs: Vec<&str> = imports.iter().map(|i| i.specifier.as_str()).collect();
        assert!(specs.contains(&"./foo.js"));
        assert!(specs.contains(&"../bar"));
        assert!(specs.contains(&"bua:fs"));
        assert!(specs.contains(&"./x"));
    }

    #[test]
    fn type_only_flagged() {
        let src = "import type { Foo } from './types';\n";
        let imports = extract_static_imports(src);
        assert!(imports.iter().any(|i| i.is_type_only));
    }

    #[test]
    fn dynamic_import_flagged() {
        let src = "const m = import('./lazy.js');\n";
        let imports = extract_static_imports(src);
        assert!(imports.iter().any(|i| i.is_dynamic));
    }

    #[test]
    fn top_level_await_detected() {
        let src = "const data = await fetch('https://example.com');\n";
        assert!(detect_top_level_await(src));
    }

    #[test]
    fn no_false_positive_in_function() {
        let src = "function f() {\n  const x = await p;\n}\n";
        assert!(!detect_top_level_await(src));
    }
}
