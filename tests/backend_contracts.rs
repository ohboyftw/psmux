use serde::{Deserialize, Serialize};

// Mirror the error code constants from src/backend/protocol.rs
const PANE_NOT_FOUND: i32 = -32001;
const SPAWN_FAILED: i32 = -32002;
const COMMAND_TIMEOUT: i32 = -32007;
const COMMAND_FAILED: i32 = -32008;

#[test]
fn test_spawn_agent_with_frontmatter_fields_deserializes() {
    let json_str = r#"{"id":"1","method":"spawn_agent","params":{
        "command":["claude","--agent"],
        "metadata":{
            "name":"worker-1",
            "color":"blue",
            "role":"researcher",
            "effort":"high",
            "max_turns":25,
            "disallowed_tools":["Bash","Write"]
        }
    }}"#;
    let req: RpcRequest =
        serde_json::from_str(json_str).expect("spawn_agent with frontmatter must deserialize");
    assert_eq!(req.method, "spawn_agent");
}

#[derive(Debug, Deserialize)]
struct RpcRequest {
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct InitializeResult {
    protocol_version: String,
    capabilities: Vec<String>,
    self_context_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SpawnAgentResult {
    context_id: String,
    ready: bool,
    elapsed_ms: u64,
    data_version: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct CaptureResult {
    text: String,
    data_version: u64,
    context_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ListResult {
    contexts: Vec<ContextInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ContextInfo {
    context_id: String,
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ContextExitedEvent {
    method: String,
    params: ContextExitedParams,
}

#[derive(Debug, Serialize, Deserialize)]
struct ContextExitedParams {
    context_id: String,
    exit_code: Option<i32>,
}

#[test]
fn test_all_rpc_methods_deserialize() {
    let cases = vec![
        (
            r#"{"id":"1","method":"initialize","params":{"protocol_version":"1","capabilities":["events"]}}"#,
            "initialize",
        ),
        (
            r#"{"id":"2","method":"spawn_agent","params":{"command":["claude","--agent"],"cwd":"/project"}}"#,
            "spawn_agent",
        ),
        (
            r#"{"id":"3","method":"write","params":{"context_id":"%1","data":"aGVsbG8="}}"#,
            "write",
        ),
        (
            r#"{"id":"4","method":"capture","params":{"context_id":"%1","lines":200}}"#,
            "capture",
        ),
        (
            r#"{"id":"5","method":"kill","params":{"context_id":"%1"}}"#,
            "kill",
        ),
        (r#"{"id":"6","method":"list","params":{}}"#, "list"),
        (r#"{"id":"7","method":"kill_all","params":{}}"#, "kill_all"),
        (
            r#"{"id":"8","method":"kill_all","params":{"role":"researcher"}}"#,
            "kill_all",
        ),
    ];
    for (json_str, expected_method) in cases {
        let req: RpcRequest = serde_json::from_str(json_str)
            .unwrap_or_else(|e| panic!("Failed to parse {expected_method}: {e}"));
        assert_eq!(req.method, expected_method);
    }
}

#[test]
fn test_initialize_response_roundtrip() {
    let result = InitializeResult {
        protocol_version: "1".into(),
        capabilities: vec!["events".into(), "capture".into()],
        self_context_id: "%0".into(),
    };
    let json = serde_json::to_string(&result).unwrap();
    let parsed: InitializeResult = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.self_context_id, "%0");
    assert_eq!(parsed.capabilities.len(), 2);
}

#[test]
fn test_context_exited_event_has_no_id() {
    let event = ContextExitedEvent {
        method: "context_exited".into(),
        params: ContextExitedParams {
            context_id: "%3".into(),
            exit_code: Some(0),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed.get("id").is_none(),
        "Push events must not have id field"
    );
    assert_eq!(parsed["method"], "context_exited");
}

#[test]
fn test_rpc_edge_cases() {
    let edge_cases = vec![
        r#"{}"#,
        r#"{"method":"initialize"}"#,
        r#"{"id":"1"}"#,
        r#"not json"#,
        r#"{"id":"1","method":"unknown","params":{}}"#,
    ];
    for input in edge_cases {
        let result = serde_json::from_str::<RpcRequest>(input);
        let _ = result;
    }
}

#[test]
fn test_capture_result_v2_fields() {
    let result = CaptureResult {
        text: "hello world".into(),
        data_version: 42,
        context_id: "%1".into(),
    };
    let json = serde_json::to_string(&result).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["text"], "hello world");
    assert_eq!(parsed["data_version"], 42);
    assert_eq!(parsed["context_id"], "%1");
    assert!(
        parsed.get("truncated").is_none(),
        "v2 CaptureResult must not have truncated field"
    );
}

#[test]
fn test_spawn_agent_result_v2_fields() {
    let result = SpawnAgentResult {
        context_id: "%2".into(),
        ready: true,
        elapsed_ms: 150,
        data_version: 1,
    };
    let json = serde_json::to_string(&result).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["context_id"], "%2");
    assert_eq!(parsed["ready"], true);
    assert_eq!(parsed["elapsed_ms"], 150);
    assert_eq!(parsed["data_version"], 1);
}

#[test]
fn spawn_result_has_protocol_v2_fields() {
    let result = SpawnAgentResult {
        context_id: "%5".into(),
        ready: true,
        elapsed_ms: 1200,
        data_version: 12,
    };
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["context_id"], "%5");
    assert_eq!(json["ready"], true);
    assert!(json["elapsed_ms"].is_number());
    assert!(json["data_version"].is_number());
}

#[derive(Debug, Serialize, Deserialize)]
struct RunShellResult {
    exit_code: i32,
    stdout: String,
    stderr: String,
    elapsed_ms: u64,
}

#[test]
fn run_shell_result_shape() {
    let result = RunShellResult {
        exit_code: 0,
        stdout: "hello\n".into(),
        stderr: String::new(),
        elapsed_ms: 42,
    };
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["exit_code"], 0);
    assert_eq!(json["stdout"], "hello\n");
    assert_eq!(json["stderr"], "");
    assert!(json["elapsed_ms"].is_number());
}

#[test]
fn error_codes_are_in_valid_range() {
    // JSON-RPC server errors: -32000 to -32099
    assert!(PANE_NOT_FOUND >= -32099 && PANE_NOT_FOUND <= -32000);
    assert!(SPAWN_FAILED >= -32099 && SPAWN_FAILED <= -32000);
    assert!(COMMAND_TIMEOUT >= -32099 && COMMAND_TIMEOUT <= -32000);
    assert!(COMMAND_FAILED >= -32099 && COMMAND_FAILED <= -32000);
}

#[test]
fn bare_flag_injects_bare_for_claude_command() {
    let mut command = vec!["claude".to_string(), "-p".to_string(), "task".to_string()];
    let bare = true;
    if bare
        && command
            .first()
            .map(|c| c.to_lowercase().contains("claude"))
            .unwrap_or(false)
    {
        command.insert(1, "--bare".to_string());
    }
    assert_eq!(command, vec!["claude", "--bare", "-p", "task"]);
}

#[test]
fn bare_flag_ignored_for_non_claude_command() {
    let mut command = vec!["python".to_string(), "script.py".to_string()];
    let bare = true;
    if bare
        && command
            .first()
            .map(|c| c.to_lowercase().contains("claude"))
            .unwrap_or(false)
    {
        command.insert(1, "--bare".to_string());
    }
    assert_eq!(command, vec!["python", "script.py"]);
}
