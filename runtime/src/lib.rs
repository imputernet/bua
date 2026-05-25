// bua-runtime: AI-native deterministic JS execution engine

// Internal JSC sys bindings — exposed crate-wide so ffi/ can use crate::jsc_sys::*
pub(crate) mod jsc_sys;

pub mod deterministic;
pub mod ffi;
pub mod globals;
pub mod metrics;
pub mod modules;
pub mod promise;
pub mod runtime;

// Core execution modules
pub mod agent;
pub mod engine;
pub mod event_loop;
pub mod loader;
pub mod permissions;
pub mod scheduler;
pub mod snapshot;
pub mod tools;
pub mod transpiler;

// Primary public API
pub use deterministic::{DeterministicClock, IoInterceptor, ReplayEngine};
pub use ffi::value::JsValue;
pub use metrics::RuntimeMetrics;
pub use modules::{BuiltinRegistry, ModuleGraph, ModuleResolver};
pub use promise::{PromiseBridge, ResolutionQueue};
pub use runtime::runtime::{Runtime, RuntimeConfig};
pub use runtime::{
    AgentContext, CapabilityContext, SnapshotContext, ToolContext, TraceContext, VmContext,
};
pub use scheduler::AgentScheduler;
pub use snapshot::{LayeredSnapshot, Snapshot};
pub use tools::{default_tool_registry, ToolCall, ToolRegistry, ToolResult};
