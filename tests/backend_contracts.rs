use serde::{Deserialize, Serialize};

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
    let req: RpcRequest = serde_json::from_str(json_str)
        .expect("spawn_agent with frontmatter must deserialize");
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
}

#[derive(Debug, Serialize, Deserialize)]
struct CaptureResult {
    text: String,
    truncated: bool,
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
        (
            r#"{"id":"7","method":"kill_all","params":{}}"#,
            "kill_all",
        ),
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
fn test_capture_result_truncated_flag() {
    let full = CaptureResult {
        text: "hello".into(),
        truncated: false,
    };
    let truncated = CaptureResult {
        text: "hel...".into(),
        truncated: true,
    };
    let full_json = serde_json::to_string(&full).unwrap();
    let trunc_json = serde_json::to_string(&truncated).unwrap();
    assert!(full_json.contains("\"truncated\":false"));
    assert!(trunc_json.contains("\"truncated\":true"));
}
