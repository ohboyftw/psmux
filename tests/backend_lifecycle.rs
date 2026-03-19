mod common;
use common::make_mock_server;

#[test]
fn test_dispatch_initialize() {
    let tx = make_mock_server();
    let input = r#"{"id":"1","method":"initialize","params":{"protocol_version":"1","capabilities":["events"]}}"#;

    let response = psmux::backend::dispatcher::dispatch_rpc(input, &tx);
    assert!(response.is_some());

    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert_eq!(parsed["id"], "1");
    assert!(parsed["result"]["self_context_id"].is_string());
    assert_eq!(parsed["result"]["protocol_version"], "1");
}

#[test]
fn test_dispatch_spawn_agent() {
    let tx = make_mock_server();
    let input = r#"{"id":"2","method":"spawn_agent","params":{"command":["claude","--agent"],"cwd":"/project"}}"#;

    let response = psmux::backend::dispatcher::dispatch_rpc(input, &tx);
    assert!(response.is_some());

    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert_eq!(parsed["id"], "2");
    assert!(parsed["result"]["context_id"].is_string());
}

#[test]
fn test_dispatch_write_valid_base64() {
    let tx = make_mock_server();
    // "aGVsbG8=" is base64 for "hello"
    let input = r#"{"id":"3","method":"write","params":{"context_id":"%1","data":"aGVsbG8="}}"#;

    let response = psmux::backend::dispatcher::dispatch_rpc(input, &tx);
    assert!(response.is_some());

    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert_eq!(parsed["id"], "3");
    assert!(parsed["result"].is_object());
    assert!(parsed.get("error").is_none());
}

#[test]
fn test_dispatch_capture() {
    let tx = make_mock_server();
    let input = r#"{"id":"4","method":"capture","params":{"context_id":"%1","lines":200}}"#;

    let response = psmux::backend::dispatcher::dispatch_rpc(input, &tx);
    assert!(response.is_some());

    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert_eq!(parsed["id"], "4");
    assert!(parsed["result"]["text"].is_string());
    assert_eq!(parsed["result"]["truncated"], false);
}

#[test]
fn test_dispatch_kill() {
    let tx = make_mock_server();
    let input = r#"{"id":"5","method":"kill","params":{"context_id":"%1"}}"#;

    let response = psmux::backend::dispatcher::dispatch_rpc(input, &tx);
    assert!(response.is_some());

    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert_eq!(parsed["id"], "5");
    assert!(parsed["result"].is_object());
}

#[test]
fn test_dispatch_list() {
    let tx = make_mock_server();
    let input = r#"{"id":"6","method":"list","params":{}}"#;

    let response = psmux::backend::dispatcher::dispatch_rpc(input, &tx);
    assert!(response.is_some());

    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert_eq!(parsed["id"], "6");
    assert!(parsed["result"]["contexts"].is_array());
    assert_eq!(parsed["result"]["contexts"].as_array().unwrap().len(), 0);
}

#[test]
fn test_dispatch_unknown_method() {
    let tx = make_mock_server();
    let input = r#"{"id":"1","method":"nonexistent","params":{}}"#;

    let response = psmux::backend::dispatcher::dispatch_rpc(input, &tx);
    assert!(response.is_some());

    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert!(parsed["error"].is_object());
    assert_eq!(parsed["error"]["code"], -32601);
}

#[test]
fn test_dispatch_malformed_json() {
    let tx = make_mock_server();
    let response = psmux::backend::dispatcher::dispatch_rpc("not json", &tx);
    assert!(response.is_some());

    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert!(parsed["error"].is_object());
    assert_eq!(parsed["error"]["code"], -32700);
}

#[test]
fn test_dispatch_write_invalid_base64() {
    let tx = make_mock_server();
    let input =
        r#"{"id":"1","method":"write","params":{"context_id":"%1","data":"!!!not-base64!!!"}}"#;

    let response = psmux::backend::dispatcher::dispatch_rpc(input, &tx);
    assert!(response.is_some());

    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert!(parsed["error"].is_object());
    assert_eq!(parsed["error"]["code"], -32602);
}

#[test]
fn test_dispatch_kill_all() {
    let tx = make_mock_server();
    let input = r#"{"id":"7","method":"kill_all","params":{}}"#;

    let response = psmux::backend::dispatcher::dispatch_rpc(input, &tx);
    assert!(response.is_some());

    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert_eq!(parsed["id"], "7");
    assert!(parsed["result"]["killed"].is_array());
    assert_eq!(parsed["result"]["killed"].as_array().unwrap().len(), 2);
}

#[test]
fn test_dispatch_spawn_agent_empty_command() {
    let tx = make_mock_server();
    let input = r#"{"id":"1","method":"spawn_agent","params":{"command":[]}}"#;

    let response = psmux::backend::dispatcher::dispatch_rpc(input, &tx);
    assert!(response.is_some());

    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert!(parsed["error"].is_object());
    assert_eq!(parsed["error"]["code"], -32602);
}

#[test]
fn test_dispatch_preserves_request_id() {
    let tx = make_mock_server();

    // String ID
    let input = r#"{"id":"abc-123","method":"list","params":{}}"#;
    let response = psmux::backend::dispatcher::dispatch_rpc(input, &tx);
    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert_eq!(parsed["id"], "abc-123");

    // Numeric ID
    let input = r#"{"id":42,"method":"list","params":{}}"#;
    let response = psmux::backend::dispatcher::dispatch_rpc(input, &tx);
    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert_eq!(parsed["id"], 42);

    // Null ID (missing)
    let input = r#"{"method":"list","params":{}}"#;
    let response = psmux::backend::dispatcher::dispatch_rpc(input, &tx);
    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert!(parsed["id"].is_null());
}
