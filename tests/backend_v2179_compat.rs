//! Integration smoke tests for Claude Code v2.1.79 backend compatibility.
//! Tests boundary contracts between Claude Code and psmux's CustomPaneBackend.

mod common;
use common::make_mock_server;

// ── Task 1: DCS passthrough default ──

/// CC v2.1.78: terminal notifications require allow-passthrough "on" by default.
#[test]
fn test_passthrough_default_is_on() {
    let app = psmux::types::AppState::new("test".to_string());
    assert_eq!(app.allow_passthrough, "on");
}

// ── Task 2: kill_all RPC ──

/// CC v2.1.77: leader exit must be able to kill all teammate panes.
/// Smoke test: kill_all RPC dispatches without error and returns expected shape.
#[test]
fn test_kill_all_boundary_contract() {
    let tx = make_mock_server();
    let input = r#"{"id":"1","method":"kill_all","params":{}}"#;
    let resp = psmux::backend::dispatcher::dispatch_rpc(input, &tx).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["error"].is_null(), "kill_all must not return error");
    assert!(v["result"]["killed"].is_array());

    let input = r#"{"id":"2","method":"kill_all","params":{"role":"researcher"}}"#;
    let resp = psmux::backend::dispatcher::dispatch_rpc(input, &tx).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["error"].is_null(), "kill_all with role filter must not return error");
}

// ── Task 3: capture clean + lines ──

/// CC sends `lines` and `clean` params in capture requests.
/// `clean` must be accepted as a field in CaptureParams.
#[test]
fn test_capture_accepts_clean_param() {
    let json = r#"{"context_id":"%1","lines":50,"clean":true}"#;
    let params: psmux::backend::protocol::CaptureParams =
        serde_json::from_str(json).unwrap();
    assert_eq!(params.lines, Some(50));
    assert_eq!(params.clean, Some(true));
}

/// Capture with clean=true and lines params dispatches without error.
#[test]
fn test_capture_clean_dispatches() {
    let tx = make_mock_server();
    let input = r#"{"id":"1","method":"capture","params":{"context_id":"%1","lines":50,"clean":true}}"#;
    let resp = psmux::backend::dispatcher::dispatch_rpc(input, &tx).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["error"].is_null(), "capture with clean+lines must not error");
    assert!(v["result"]["text"].is_string());
}

// ── Task 4: AgentMetadata with frontmatter fields + color ──

/// CC v2.1.78: agent frontmatter fields must be captured in AgentMetadata.
#[test]
fn test_agent_metadata_captures_frontmatter() {
    let json = r#"{"name":"w1","effort":"high","max_turns":25,"disallowed_tools":["Bash"]}"#;
    let meta: psmux::backend::protocol::AgentMetadata =
        serde_json::from_str(json).unwrap();
    assert_eq!(meta.effort, Some("high".into()));
    assert_eq!(meta.max_turns, Some(25));
    assert_eq!(meta.disallowed_tools, Some(vec!["Bash".to_string()]));
}

/// CC sends color in agent metadata; list response must include it.
#[test]
fn test_list_returns_color_in_metadata() {
    let info = psmux::backend::protocol::ContextInfo {
        context_id: "%1".into(),
        metadata: Some(psmux::backend::protocol::AgentMetadata {
            name: Some("worker-1".into()),
            color: Some("blue".into()),
            role: Some("researcher".into()),
            effort: None,
            max_turns: None,
            disallowed_tools: None,
        }),
    };
    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains(r#""color":"blue""#));
}

/// spawn_agent with all frontmatter fields dispatches without error.
#[test]
fn test_spawn_agent_with_frontmatter_dispatches() {
    let tx = make_mock_server();
    let input = r#"{"id":"1","method":"spawn_agent","params":{
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
    let resp = psmux::backend::dispatcher::dispatch_rpc(input, &tx).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["error"].is_null(), "spawn_agent with frontmatter must not error");
}

// ── Task 5: split_direction ──

/// spawn_agent must accept split_direction param.
#[test]
fn test_spawn_agent_accepts_split_direction() {
    let json = r#"{"command":["claude"],"split_direction":"horizontal"}"#;
    let params: psmux::backend::protocol::SpawnAgentParams =
        serde_json::from_str(json).unwrap();
    assert_eq!(params.split_direction, Some("horizontal".into()));
}

/// spawn_agent with invalid split_direction returns error.
#[test]
fn test_spawn_agent_rejects_invalid_split_direction() {
    let tx = make_mock_server();
    let input = r#"{"id":"1","method":"spawn_agent","params":{"command":["claude"],"split_direction":"diagonal"}}"#;
    let resp = psmux::backend::dispatcher::dispatch_rpc(input, &tx).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["error"]["code"], -32602, "invalid split_direction must return -32602");
}

/// spawn_agent with split_direction dispatches without error.
#[test]
fn test_spawn_agent_split_direction_dispatches() {
    let tx = make_mock_server();
    let input = r#"{"id":"1","method":"spawn_agent","params":{"command":["claude"],"split_direction":"horizontal"}}"#;
    let resp = psmux::backend::dispatcher::dispatch_rpc(input, &tx).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["error"].is_null(), "spawn_agent with split_direction must not error");
}

// ── Task 6: Graceful kill ──

/// CC v2.1.76: kill RPC must accept a grace_ms param.
#[test]
fn test_kill_accepts_grace_period() {
    let json = r#"{"context_id":"%1","grace_ms":3000}"#;
    let params: psmux::backend::protocol::KillParams =
        serde_json::from_str(json).unwrap();
    assert_eq!(params.grace_ms, Some(3000));
}

/// kill with grace_ms dispatches without error.
#[test]
fn test_kill_with_grace_ms_dispatches() {
    let tx = make_mock_server();
    let input = r#"{"id":"1","method":"kill","params":{"context_id":"%1","grace_ms":3000}}"#;
    let resp = psmux::backend::dispatcher::dispatch_rpc(input, &tx).unwrap();
    let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
    assert!(v["error"].is_null(), "kill with grace_ms must not error");
}

// ── Task 7: Env var propagation ──

/// CC v2.1.74-v2.1.79: verify env var propagation list is in source code.
/// Grep the pane.rs source for each required env var name.
#[test]
fn test_new_env_vars_in_propagation_code() {
    let pane_source = std::fs::read_to_string("src/pane.rs")
        .expect("src/pane.rs must be readable from the test working directory");
    let required_vars = [
        "CLAUDE_CODE_SESSIONEND_HOOKS_TIMEOUT_MS",
        "CLAUDE_PLUGIN_DATA",
        "CLAUDE_CODE_PLUGIN_SEED_DIR",
        "CLAUDE_CODE_DISABLE_TERMINAL_TITLE",
        "CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS",
        "ANTHROPIC_CUSTOM_MODEL_OPTION",
    ];
    for var in &required_vars {
        assert!(
            pane_source.contains(var),
            "src/pane.rs must propagate env var {var} to agent panes (CC v2.1.74-v2.1.79)"
        );
    }
}

// ── TMUX env var format ──

/// TMUX env var must use tmux-compatible format.
#[test]
fn test_tmux_env_var_format() {
    let pane_source = std::fs::read_to_string("src/pane.rs")
        .expect("src/pane.rs must be readable");
    assert!(
        pane_source.contains("/tmp/tmux-"),
        "TMUX env var must use /tmp/tmux-* format for CC detection compatibility"
    );
}
