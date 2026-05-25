// bua-runtime: AI-native deterministic JS execution engine

// Internal JSC sys bindings — exposed crate-wide so ffi/ can use crate::jsc_sys::*
pub(crate) mod jsc_sys;

pub mod ffi;
pub mod runtime;
pub mod promise;
pub mod deterministic;
pub mod modules;
pub mod globals;
pub mod metrics;

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
pub use runtime::runtime::{Runtime, RuntimeConfig};
pub use runtime::{AgentContext, CapabilityContext, SnapshotContext, ToolContext, TraceContext, VmContext};
pub use ffi::value::JsValue;
pub use tools::{ToolCall, ToolRegistry, ToolResult, default_tool_registry};
pub use scheduler::AgentScheduler;
pub use snapshot::{LayeredSnapshot, Snapshot};
pub use promise::{PromiseBridge, ResolutionQueue};
pub use deterministic::{DeterministicClock, ReplayEngine, IoInterceptor};
pub use modules::{ModuleGraph, ModuleResolver, BuiltinRegistry};
pub use metrics::RuntimeMetrics;
