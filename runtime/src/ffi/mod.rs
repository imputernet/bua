// runtime/src/ffi/mod.rs
//
// The ONLY place in bua-runtime that knows about raw JSC pointers.
// Every other module talks to opaque handles and safe Rust types.
//
// Rule: no *mut, no unsafe, no JSC types outside this module tree.

pub mod context;
pub mod value;
pub mod engine;

pub use context::JscContext;
pub use value::{JsValue, ObjectHandle, ArrayHandle, FunctionHandle};
pub use engine::JscEngine;
