use thiserror::Error;

pub type BuaResult<T> = Result<T, BuaError>;

#[derive(Debug, Error)]
pub enum BuaError {
    // --- Permission errors ---
    #[error("permission denied: {operation} requires capability {capability}")]
    PermissionDenied {
        operation: String,
        capability: String,
    },

    // --- JS engine errors ---
    #[error("js exception: {message}")]
    JsException {
        message: String,
        stack: Option<String>,
    },

    #[error("js engine init failed: {0}")]
    JsEngineInit(String),

    // --- Module loading ---
    #[error("module not found: {specifier}")]
    ModuleNotFound { specifier: String },

    #[error("module load failed: {specifier}: {reason}")]
    ModuleLoadFailed { specifier: String, reason: String },

    // --- Agent errors ---
    #[error("agent {id} not found")]
    AgentNotFound { id: String },

    #[error("agent {id} spawn failed: {reason}")]
    AgentSpawnFailed { id: String, reason: String },

    #[error("agent {id} timed out after {timeout_ms}ms")]
    AgentTimeout { id: String, timeout_ms: u64 },

    // --- Tool errors ---
    #[error("tool {name} not registered")]
    ToolNotFound { name: String },

    #[error("tool {name} call failed: {reason}")]
    ToolCallFailed { name: String, reason: String },

    // --- Snapshot/replay ---
    #[error("snapshot serialize failed: {0}")]
    SnapshotSerialize(String),

    #[error("snapshot restore failed: {0}")]
    SnapshotRestore(String),

    // --- I/O ---
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    // --- Serialization ---
    #[error("serialization error: {0}")]
    Serialize(#[from] serde_json::Error),

    // --- Internal ---
    #[error("internal error: {0}")]
    Internal(String),
}

impl BuaError {
    pub fn permission_denied(operation: impl Into<String>, capability: impl Into<String>) -> Self {
        Self::PermissionDenied {
            operation: operation.into(),
            capability: capability.into(),
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    pub fn is_permission_error(&self) -> bool {
        matches!(self, BuaError::PermissionDenied { .. })
    }
}
