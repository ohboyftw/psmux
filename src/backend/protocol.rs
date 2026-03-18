use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetadata {
    pub name: Option<String>,
    pub color: Option<String>,
    pub role: Option<String>,
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
}

#[derive(Debug, Deserialize)]
pub struct KillParams {
    pub context_id: String,
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
}

#[derive(Debug, Serialize)]
pub struct CaptureResult {
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct ListResult {
    pub contexts: Vec<ContextInfo>,
}

#[derive(Debug, Serialize)]
pub struct ContextInfo {
    pub context_id: String,
    pub metadata: Option<AgentMetadata>,
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
            }),
        }
    }
}
