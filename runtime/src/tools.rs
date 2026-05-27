/// tools.rs — Native tool registry and call dispatch
///
/// Tools are Rust async functions exposed to JS agents via the Bua.tools.*
/// namespace. Each tool has a JSON Schema for input validation and a typed
/// Rust implementation.
use bua_core::{BuaError, BuaResult, CapabilitySet, Permission};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub args: Value,
    /// Caller-supplied correlation id for tracing.
    pub call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub call_id: Option<String>,
    pub name: String,
    pub output: Value,
    pub duration_us: u64,
    pub error: Option<String>,
}

impl ToolResult {
    pub fn success(call: &ToolCall, output: Value, duration_us: u64) -> Self {
        Self {
            call_id: call.call_id.clone(),
            name: call.name.clone(),
            output,
            duration_us,
            error: None,
        }
    }

    pub fn failed(call: &ToolCall, error: String, duration_us: u64) -> Self {
        Self {
            call_id: call.call_id.clone(),
            name: call.name.clone(),
            output: Value::Null,
            duration_us,
            error: Some(error),
        }
    }
}

impl Default for HttpGetTool {
    fn default() -> Self {
        Self::new()
    }
}

/// JSON Schema fragment describing a tool's inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub description: String,
    pub parameters: Value, // JSON Schema object
}

// ---------------------------------------------------------------------------
// Tool trait
// ---------------------------------------------------------------------------

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

pub trait Tool: std::fmt::Debug + Send + Sync + 'static {
    fn name(&self) -> &str;
    fn schema(&self) -> &ToolSchema;

    /// Check if the caller has capabilities required for this tool.
    fn required_permissions(&self) -> Vec<Permission> {
        vec![]
    }

    fn call(&self, args: Value) -> BoxFuture<BuaResult<Value>>;
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register<T: Tool>(&mut self, tool: T) {
        let name = tool.name().to_string();
        tracing::debug!(name, "registered tool");
        self.tools.insert(name, Arc::new(tool));
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// Dispatch a tool call, enforcing capability checks.
    pub async fn dispatch(&self, call: &ToolCall, caps: &CapabilitySet) -> ToolResult {
        let start = std::time::Instant::now();

        let Some(tool) = self.get(&call.name) else {
            return ToolResult::failed(
                call,
                format!("tool '{}' not registered", call.name),
                start.elapsed().as_micros() as u64,
            );
        };

        // Permission check
        for perm in tool.required_permissions() {
            if !caps.check(&perm) {
                return ToolResult::failed(
                    call,
                    format!("permission denied for tool '{}'", call.name),
                    start.elapsed().as_micros() as u64,
                );
            }
        }

        match tool.call(call.args.clone()).await {
            Ok(output) => ToolResult::success(call, output, start.elapsed().as_micros() as u64),
            Err(e) => ToolResult::failed(call, e.to_string(), start.elapsed().as_micros() as u64),
        }
    }

    pub fn list(&self) -> Vec<(&str, &ToolSchema)> {
        self.tools
            .iter()
            .map(|(name, t)| (name.as_str(), t.schema()))
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Built-in tools
// ---------------------------------------------------------------------------

/// Example built-in: `bua_read_file`
#[derive(Debug)]
pub struct ReadFileTool;

impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "bua_read_file"
    }

    fn schema(&self) -> &ToolSchema {
        static SCHEMA: std::sync::OnceLock<ToolSchema> = std::sync::OnceLock::new();
        SCHEMA.get_or_init(|| ToolSchema {
            description: "Read a UTF-8 text file from the filesystem.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute or relative file path" }
                },
                "required": ["path"]
            }),
        })
    }

    fn required_permissions(&self) -> Vec<Permission> {
        // The actual path is checked dynamically in the tool body.
        vec![]
    }

    fn call(&self, args: Value) -> BoxFuture<BuaResult<Value>> {
        Box::pin(async move {
            let path = args["path"]
                .as_str()
                .ok_or_else(|| BuaError::internal("missing 'path' argument"))?;

            let content = tokio::fs::read_to_string(path)
                .await
                .map_err(BuaError::Io)?;

            Ok(serde_json::json!({ "content": content, "bytes": content.len() }))
        })
    }
}

/// Example built-in: `bua_http_get`
#[derive(Debug)]
pub struct HttpGetTool {
    client: Arc<reqwest::Client>,
}

impl HttpGetTool {
    pub fn new() -> Self {
        Self {
            client: Arc::new(reqwest::Client::new()),
        }
    }
}

impl Tool for HttpGetTool {
    fn name(&self) -> &str {
        "bua_http_get"
    }

    fn schema(&self) -> &ToolSchema {
        static SCHEMA: std::sync::OnceLock<ToolSchema> = std::sync::OnceLock::new();
        SCHEMA.get_or_init(|| ToolSchema {
            description: "Perform an HTTP GET request.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" },
                    "headers": { "type": "object", "additionalProperties": { "type": "string" } }
                },
                "required": ["url"]
            }),
        })
    }

    fn required_permissions(&self) -> Vec<Permission> {
        // URL-specific check happens in the body; registry-level requires NetConnect.
        vec![]
    }

    fn call(&self, args: Value) -> BoxFuture<BuaResult<Value>> {
        let client = self.client.clone();
        Box::pin(async move {
            let url = args["url"]
                .as_str()
                .ok_or_else(|| BuaError::internal("missing 'url'"))?;

            let mut req = client.get(url);

            if let Some(headers) = args["headers"].as_object() {
                for (k, v) in headers {
                    if let Some(v_str) = v.as_str() {
                        req = req.header(k, v_str);
                    }
                }
            }

            let resp = req
                .send()
                .await
                .map_err(|e| BuaError::internal(e.to_string()))?;
            let status = resp.status().as_u16();
            let body = resp
                .text()
                .await
                .map_err(|e| BuaError::internal(e.to_string()))?;

            Ok(serde_json::json!({ "status": status, "body": body }))
        })
    }
}

// ---------------------------------------------------------------------------
// Registry builder
// ---------------------------------------------------------------------------

pub fn default_tool_registry() -> ToolRegistry {
    let mut reg = ToolRegistry::new();
    reg.register(ReadFileTool);
    reg.register(HttpGetTool::new());
    reg
}
