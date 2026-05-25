use crate::types::{AgentId, ExecutionId, TaskId};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_micros() as u64
}

/// A single immutable event in an execution trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    pub id: u64,
    pub timestamp_us: u64,
    pub execution_id: ExecutionId,
    pub kind: TraceEventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceEventKind {
    ExecutionStart {
        entrypoint: String,
    },
    ExecutionEnd {
        exit_code: i32,
        duration_us: u64,
    },
    AgentSpawn {
        agent_id: AgentId,
        parent_id: Option<AgentId>,
        entrypoint: String,
    },
    AgentComplete {
        agent_id: AgentId,
        task_id: TaskId,
        success: bool,
    },
    ToolCall {
        name: String,
        args_json: String,
    },
    ToolResult {
        name: String,
        result_json: String,
        duration_us: u64,
    },
    PermissionCheck {
        operation: String,
        granted: bool,
    },
    JsException {
        message: String,
        stack: Option<String>,
    },
    Log {
        level: LogLevel,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// Append-only trace buffer for a single execution.
#[derive(Debug, Default)]
pub struct ExecutionTrace {
    events: Vec<TraceEvent>,
    next_id: u64,
    execution_id: Option<ExecutionId>,
}

impl ExecutionTrace {
    pub fn new(execution_id: ExecutionId) -> Self {
        Self {
            events: Vec::with_capacity(64),
            next_id: 0,
            execution_id: Some(execution_id),
        }
    }

    pub fn append(&mut self, kind: TraceEventKind) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.events.push(TraceEvent {
            id,
            timestamp_us: now_us(),
            execution_id: self.execution_id.clone().unwrap_or_default(),
            kind,
        });
        id
    }

    pub fn events(&self) -> &[TraceEvent] {
        &self.events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Serialize the full trace to newline-delimited JSON (NDJSON).
    pub fn to_ndjson(&self) -> String {
        self.events
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .collect::<Vec<_>>()
            .join("\n")
    }
}
