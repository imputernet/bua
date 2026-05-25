// runtime/src/modules/mod.rs
//
// Full ESM module evaluation pipeline.
//
// Keeps eval() and eval_module() on separate paths — they are fundamentally
// different: eval() is a script execution, eval_module() is a graph traversal.
//
// Module graph evaluation order:
//   1. resolve(specifier, referrer) -> AbsolutePath
//   2. load(path) -> RawSource
//   3. transform(source) -> JsSource (TypeScript strip, source map)
//   4. parse(source) -> ModuleRecord (imports extracted)
//   5. link(record) -> resolve all imports recursively (cycle detection)
//   6. evaluate(record) -> run top-level code, handle top-level await
//
// Cycle detection: DFS with a "currently linking" color set.
// Top-level await: module evaluation returns a Promise; engine awaits it.

pub mod graph;
pub mod record;
pub mod resolver;
pub mod builtins;

pub use graph::{ModuleGraph, EvalOrder};
pub use record::{ModuleRecord, ModuleStatus};
pub use resolver::ModuleResolver;
pub use builtins::BuiltinRegistry;
