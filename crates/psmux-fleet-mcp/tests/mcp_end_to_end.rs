//! Spawns the MCP binary, sends a `fleet.list` request over stdio, reads the
//! response, asserts the workspace count.

use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_psmux-fleet-mcp")
}

#[test]
fn lists_workspaces_from_seeded_state() {
    let td = TempDir::new().unwrap();
    let state = td.path().join("state.json");
    std::fs::write(
        &state,
        include_str!("../../psmux-fleet/fixtures/state_healthy.json"),
    )
    .unwrap();

    let mut child = Command::new(bin())
        .args(["--state-path", state.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Minimal JSON-RPC call — hand-rolled framing (one request per line).
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "fleet.list",
        "params": {}
    });
    writeln!(child.stdin.as_mut().unwrap(), "{req}").unwrap();
    drop(child.stdin.take()); // EOF signals the server to exit.

    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .expect("expected at least one response line")
        .to_string();
    let resp: serde_json::Value = serde_json::from_str(&line).expect("parse JSON-RPC response");
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["total"], 3);
    let workspaces = resp["result"]["workspaces"]
        .as_array()
        .expect("workspaces array");
    assert_eq!(workspaces.len(), 3);
}

#[test]
fn status_returns_workspace_detail() {
    let td = TempDir::new().unwrap();
    let state = td.path().join("state.json");
    std::fs::write(
        &state,
        include_str!("../../psmux-fleet/fixtures/state_healthy.json"),
    )
    .unwrap();

    let mut child = Command::new(bin())
        .args(["--state-path", state.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "fleet.status",
        "params": { "id": "lint-docs" }
    });
    writeln!(child.stdin.as_mut().unwrap(), "{req}").unwrap();
    drop(child.stdin.take());

    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap()
        .to_string();
    let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(resp["result"]["id"], "lint-docs");
    assert_eq!(resp["result"]["status"], "succeeded");
}

#[test]
fn unknown_method_returns_error_envelope() {
    let td = TempDir::new().unwrap();
    let state = td.path().join("state.json");
    std::fs::write(
        &state,
        include_str!("../../psmux-fleet/fixtures/state_healthy.json"),
    )
    .unwrap();

    let mut child = Command::new(bin())
        .args(["--state-path", state.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "fleet.nope",
        "params": {}
    });
    writeln!(child.stdin.as_mut().unwrap(), "{req}").unwrap();
    drop(child.stdin.take());

    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap()
        .to_string();
    let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(resp["error"]["code"], "METHOD_NOT_FOUND");
}
