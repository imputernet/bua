// runtime/src/runtime/mod.rs
//
// The Runtime context hierarchy.
//
// Every agent gets an isolated Runtime instance containing:
//
//   Runtime
//    ├── VmContext        — JS engine handle + module loader
//    ├── AgentContext     — agent identity, parent, lifecycle state
//    ├── CapabilityContext — live capability set + permission guard
//    ├── ToolContext      — tool registry view + call dispatch
//    ├── TraceContext     — structured execution trace (append-only)
//    └── SnapshotContext  — checkpoint / restore state
//
// Contexts are composed, not inherited. A Runtime is cheap to clone
// (all inner state is Arc-wrapped). Cloning = sharing the same execution
// context, which is intentional for sub-tasks within one agent.

pub mod agent_ctx;
pub mod capability_ctx;
#[allow(clippy::module_inception)]
pub mod runtime;
pub mod snapshot_ctx;
pub mod tool_ctx;
pub mod trace_ctx;
pub mod vm;

pub use agent_ctx::AgentContext;
pub use capability_ctx::CapabilityContext;
pub use runtime::Runtime;
pub use snapshot_ctx::SnapshotContext;
pub use tool_ctx::ToolContext;
pub use trace_ctx::TraceContext;
pub use vm::VmContext;
