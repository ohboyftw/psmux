use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Error Codes ──
pub const PANE_NOT_FOUND: i32 = -32001;
pub const SPAWN_FAILED: i32 = -32002;
pub const PANE_TOO_SMALL: i32 = -32003;
pub const SPAWN_TIMEOUT: i32 = -32004;
pub const CAPTURE_TIMEOUT: i32 = -32005;
pub const SESSION_NOT_FOUND: i32 = -32006;
pub const COMMAND_TIMEOUT: i32 = -32007;
pub const COMMAND_FAILED: i32 = -32008;

// --- Requests ---

#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct InitializeParams {
    pub protocol_version: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SpawnAgentParams {
    pub command: Vec<String>,
    pub cwd: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub metadata: Option<AgentMetadata>,
    pub split_direction: Option<String>,
    /// Spawn mode: "split" (pane split), "window" (new window), "auto" (split
    /// with fallback to window on pane-too-small).  Default is "auto".
    pub mode: Option<String>,
    /// Name for the new window (only used when mode is "window" or auto-fallback).
    pub window_name: Option<String>,
    #[serde(default = "default_true")]
    pub wait_ready: bool,
    pub ready_timeout_ms: Option<u32>,
    pub shell: Option<String>,
    pub bare: Option<bool>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetadata {
    pub name: Option<String>,
    pub color: Option<String>,
    pub role: Option<String>,
    pub effort: Option<String>,
    pub max_turns: Option<u32>,
    pub disallowed_tools: Option<Vec<String>>,
}

impl AgentMetadata {
    /// Write this metadata into a pane's `HashMap<String, String>`.
    pub fn apply_to(&self, map: &mut std::collections::HashMap<String, String>) {
        if let Some(ref v) = self.name {
            map.insert("@agent".into(), v.clone());
        }
        if let Some(ref v) = self.role {
            map.insert("@role".into(), v.clone());
        }
        if let Some(ref v) = self.color {
            map.insert("@color".into(), v.clone());
        }
        if let Some(ref v) = self.effort {
            map.insert("@effort".into(), v.clone());
        }
        if let Some(v) = self.max_turns {
            map.insert("@max_turns".into(), v.to_string());
        }
        if let Some(ref v) = self.disallowed_tools {
            map.insert("@disallowed_tools".into(), v.join(","));
        }
    }

    /// Reconstruct from a pane's metadata map.  Returns `None` if the map is empty.
    pub fn from_metadata_map(map: &std::collections::HashMap<String, String>) -> Option<Self> {
        if map.is_empty() {
            return None;
        }
        Some(Self {
            name: map.get("@agent").cloned(),
            color: map.get("@color").cloned(),
            role: map.get("@role").cloned(),
            effort: map.get("@effort").cloned(),
            max_turns: map.get("@max_turns").and_then(|v| v.parse().ok()),
            // An empty list joins to "", and splitting "" yields one empty
            // element — so without the filter, "nothing is disallowed" comes
            // back as "the tool named empty-string is disallowed".
            disallowed_tools: map.get("@disallowed_tools").map(|v| {
                v.split(',')
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            }),
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct WriteParams {
    pub context_id: String,
    pub data: String, // base64 encoded
}

#[derive(Debug, Deserialize)]
pub struct CaptureParams {
    pub context_id: String,
    pub lines: Option<u32>,
    pub clean: Option<bool>,
    #[serde(default)]
    pub wait_for_output: bool,
    pub since_version: Option<u64>,
    pub timeout_ms: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct KillParams {
    pub context_id: String,
    pub grace_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct SetMetadataParams {
    pub context_id: String,
    pub metadata: AgentMetadata,
}

#[derive(Debug, Deserialize)]
pub struct KillAllParams {
    pub role: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RunShellParams {
    pub command: Vec<String>,
    pub cwd: Option<String>,
    pub context_id: Option<String>,
    pub timeout_ms: Option<u32>,
    pub env: Option<HashMap<String, String>>,
}

// --- Responses ---

#[derive(Debug, Serialize)]
pub struct RpcResponse {
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: Vec<String>,
    pub self_context_id: String,
}

#[derive(Debug, Serialize)]
pub struct SpawnAgentResult {
    pub context_id: String,
    pub ready: bool,
    pub elapsed_ms: u64,
    pub data_version: u64,
    /// How the pane was created: "split" or "window".
    pub created_via: String,
}

#[derive(Debug, Serialize)]
pub struct CaptureResult {
    pub text: String,
    pub data_version: u64,
    pub context_id: String,
}

#[derive(Debug, Serialize)]
pub struct RunShellResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct ExecParams {
    pub context_id: Option<String>,
    pub command: String,
    #[serde(default)]
    pub capture: bool,
    pub timeout_ms: Option<u64>,
    pub shell: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WaitForParams {
    /// One of: `"exit"`, `"file"`, `"output"`, `"ready"`.
    pub condition: String,
    /// Condition-specific argument (PID for exit, path for file, regex for output).
    /// Ignored for `ready`.
    pub arg: Option<String>,
    /// `%N`-style pane context for `output` / `ready` modes.
    pub pane_id: Option<String>,
    /// Wait ceiling. Defaults to 1 hour.
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ExecResult {
    pub exit_code: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct ListResult {
    pub contexts: Vec<ContextInfo>,
}

#[derive(Debug, Serialize)]
pub struct ContextInfo {
    pub context_id: String,
    pub alive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_name: Option<String>,
    pub metadata: Option<AgentMetadata>,
}

#[derive(Debug, Serialize)]
pub struct KillAllResult {
    pub killed: Vec<String>,
}

// --- Push Events ---

#[derive(Debug, Serialize)]
pub struct ContextExitedEvent {
    pub method: String, // always "context_exited"
    pub params: ContextExitedParams,
}

#[derive(Debug, Serialize)]
pub struct ContextExitedParams {
    pub context_id: String,
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ContextReadyEvent {
    pub method: String,
    pub params: ContextReadyParams,
}

#[derive(Debug, Serialize)]
pub struct ContextReadyParams {
    pub context_id: String,
    pub ready_signal: String,
    pub data_version: u64,
}

#[derive(Debug, Serialize)]
pub struct ExecCompletedEvent {
    pub method: String,
    pub params: ExecCompletedParams,
}

#[derive(Debug, Serialize)]
pub struct ExecCompletedParams {
    pub context_id: String,
    pub exit_code: i32,
    pub command: String,
    pub elapsed_ms: u64,
}

impl RpcResponse {
    pub fn success(id: serde_json::Value, result: impl Serialize) -> Self {
        Self {
            id,
            result: Some(serde_json::to_value(result).unwrap_or_default()),
            error: None,
        }
    }

    pub fn error(id: serde_json::Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }

    pub fn error_with_data(
        id: serde_json::Value,
        code: i32,
        message: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self {
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: Some(data),
            }),
        }
    }
}
