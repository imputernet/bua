// runtime/src/modules/resolver.rs

use bua_core::{BuaError, BuaResult};
use std::path::{Path, PathBuf};

/// Resolution context for a single specifier lookup.
#[derive(Debug, Clone)]
pub struct ResolveContext {
    /// The file that contains the import statement (None for entrypoint).
    pub referrer: Option<PathBuf>,
    /// Root directory of the agent execution.
    pub root: PathBuf,
}

/// The result of resolving a specifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedSpecifier {
    /// A file on disk with an absolute path.
    File(PathBuf),
    /// A Bua builtin module (`bua:fs`, `bua:env`, etc.)
    Builtin(String),
}

/// Resolves module specifiers to absolute paths.
///
/// Resolution algorithm:
///   1. `bua:*` -> Builtin
///   2. Relative (./  ../) -> probe extensions relative to referrer
///   3. Absolute (/) -> probe extensions
///   4. Bare specifier -> error (no node_modules in Bua)
#[allow(dead_code)]
pub struct ModuleResolver {
    root: PathBuf,
}

const PROBE_EXTENSIONS: &[&str] = &[
    "", // exact match
    ".ts", ".js", ".mts", ".mjs", ".tsx", ".jsx",
];

const INDEX_FILES: &[&str] = &["index.ts", "index.js", "index.mts", "index.mjs"];

impl ModuleResolver {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn resolve(&self, specifier: &str, ctx: &ResolveContext) -> BuaResult<ResolvedSpecifier> {
        // 1. Bua builtins
        if let Some(name) = specifier.strip_prefix("bua:") {
            return Ok(ResolvedSpecifier::Builtin(name.to_string()));
        }

        // 2. Relative
        if specifier.starts_with("./") || specifier.starts_with("../") {
            let base = ctx
                .referrer
                .as_ref()
                .and_then(|p| p.parent())
                .unwrap_or(&ctx.root);
            let candidate = base.join(specifier);
            return self.probe(&candidate);
        }

        // 3. Absolute
        if specifier.starts_with('/') {
            return self.probe(Path::new(specifier));
        }

        // 4. Bare specifier — not supported (no npm in Bua)
        Err(BuaError::ModuleNotFound {
            specifier: format!(
                "'{specifier}' — bare specifiers are not supported. \
                 Use relative paths or 'bua:*' builtins."
            ),
        })
    }

    fn probe(&self, base: &Path) -> BuaResult<ResolvedSpecifier> {
        // Try exact path or path + extension
        for ext in PROBE_EXTENSIONS {
            let candidate = if ext.is_empty() {
                base.to_path_buf()
            } else {
                let name = base
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let mut p = base.to_path_buf();
                p.set_file_name(format!("{name}{ext}"));
                p
            };

            if candidate.is_file() {
                return Ok(ResolvedSpecifier::File(
                    candidate.canonicalize().unwrap_or(candidate),
                ));
            }

            // Directory with index file
            if candidate.is_dir() {
                for idx in INDEX_FILES {
                    let idx_path = candidate.join(idx);
                    if idx_path.is_file() {
                        return Ok(ResolvedSpecifier::File(
                            idx_path.canonicalize().unwrap_or(idx_path),
                        ));
                    }
                }
            }
        }

        Err(BuaError::ModuleNotFound {
            specifier: base.to_string_lossy().into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn resolver_in(dir: &Path) -> ModuleResolver {
        ModuleResolver::new(dir.to_path_buf())
    }

    fn ctx(root: &Path, referrer: Option<&Path>) -> ResolveContext {
        ResolveContext {
            referrer: referrer.map(PathBuf::from),
            root: root.to_path_buf(),
        }
    }

    #[test]
    fn builtin_resolves() {
        let dir = TempDir::new().unwrap();
        let r = resolver_in(dir.path());
        let result = r.resolve("bua:fs", &ctx(dir.path(), None)).unwrap();
        assert_eq!(result, ResolvedSpecifier::Builtin("fs".into()));
    }

    #[test]
    fn relative_ts_resolves() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("utils.ts");
        std::fs::write(&file, "export const x = 1;").unwrap();
        let referrer = dir.path().join("main.ts");

        let r = resolver_in(dir.path());
        let result = r
            .resolve("./utils", &ctx(dir.path(), Some(&referrer)))
            .unwrap();
        assert!(matches!(result, ResolvedSpecifier::File(_)));
    }

    #[test]
    fn bare_specifier_errors() {
        let dir = TempDir::new().unwrap();
        let r = resolver_in(dir.path());
        let result = r.resolve("lodash", &ctx(dir.path(), None));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("bare specifiers"));
    }

    #[test]
    fn index_file_in_directory() {
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("utils");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(subdir.join("index.ts"), "export {}").unwrap();

        let referrer = dir.path().join("main.ts");
        let r = resolver_in(dir.path());
        let result = r
            .resolve("./utils", &ctx(dir.path(), Some(&referrer)))
            .unwrap();
        assert!(matches!(result, ResolvedSpecifier::File(_)));
    }
}
