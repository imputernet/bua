// bua-core: shared primitives, capability types, error taxonomy

pub mod capabilities;
pub mod error;
pub mod trace;
pub mod types;

pub use capabilities::{Capability, CapabilitySet, Permission};
pub use error::{BuaError, BuaResult};
pub use types::{AgentId, ExecutionId, TaskId};
