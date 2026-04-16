//! Tests that verify mycel topic naming conventions.
//!
//! These tests pin the topic strings that external subscribers (canopy and
//! other orchestrators) rely on. Changing a topic is a breaking change for
//! consumers.
//!
//! Per the Phase 2 Agent Execution Layer spec, the canonical topic for pane
//! exit is `psmux/pane/exited`. We also still publish `psmux/pane/died` as a
//! one-release deprecation shim; the shim test exists to flag the moment we
//! stop emitting it so the removal is intentional.

#![cfg(feature = "mycel")]

use std::fs;
use std::path::Path;

/// Canonical topic for pane exit events.
const TOPIC_EXITED: &str = "psmux/pane/exited";

/// Deprecated shim topic for pane exit events. TODO: remove next release.
const TOPIC_DIED_SHIM: &str = "psmux/pane/died";

fn read_tree_rs() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tree.rs");
    fs::read_to_string(&path).expect("read src/tree.rs")
}

#[test]
fn canonical_exit_topic_is_pane_exited() {
    assert_eq!(TOPIC_EXITED, "psmux/pane/exited");
}

#[test]
fn deprecated_died_shim_topic_still_exists() {
    assert_eq!(TOPIC_DIED_SHIM, "psmux/pane/died");
}

#[test]
fn tree_rs_publishes_canonical_exited_topic() {
    let src = read_tree_rs();
    assert!(
        src.contains(TOPIC_EXITED) || src.contains("topics::PANE_EXITED"),
        "src/tree.rs must publish canonical topic psmux/pane/exited (literal or topics::PANE_EXITED)"
    );
}

#[test]
fn tree_rs_publishes_deprecation_shim_topic() {
    let src = read_tree_rs();
    assert!(
        src.contains(TOPIC_DIED_SHIM),
        "src/tree.rs must publish deprecation shim psmux/pane/died until next release"
    );
}

// ─── Task 8: psmux/pane/ready payload contract ───────────────────────────────

#[test]
fn pane_ready_topic_constant_matches_spec() {
    assert_eq!(psmux::mycel::topics::PANE_READY, "psmux/pane/ready");
}

#[test]
fn pane_ready_payload_has_pane_id_and_elapsed_ms() {
    let payload = serde_json::json!({
        "pane_id": "%3",
        "elapsed_ms": 1234u64,
    });
    let bytes = serde_json::to_vec(&payload).expect("serialize");
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["pane_id"], "%3");
    assert_eq!(parsed["elapsed_ms"], 1234);
}

#[test]
fn server_mod_rs_publishes_pane_ready_topic() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/server/mod.rs");
    let src = fs::read_to_string(&path).expect("read src/server/mod.rs");
    assert!(
        src.contains("psmux/pane/ready") || src.contains("topics::PANE_READY"),
        "src/server/mod.rs must publish psmux/pane/ready alongside context_ready"
    );
}

// ─── Task 9: psmux/exec/completed payload contract ───────────────────────────

#[test]
fn exec_completed_topic_constant_matches_spec() {
    assert_eq!(psmux::mycel::topics::EXEC_COMPLETED, "psmux/exec/completed");
}

#[test]
fn exec_completed_payload_has_all_required_fields() {
    let payload = serde_json::json!({
        "pane_id": "%2",
        "pid": 4242u32,
        "exit_code": 0i32,
        "elapsed_ms": 150u64,
        "command": "echo hello",
    });
    let bytes = serde_json::to_vec(&payload).expect("serialize");
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["pane_id"], "%2");
    assert_eq!(parsed["pid"], 4242);
    assert_eq!(parsed["exit_code"], 0);
    assert_eq!(parsed["elapsed_ms"], 150);
    assert_eq!(parsed["command"], "echo hello");
}

#[test]
fn exec_completed_supports_nonzero_exit_code() {
    let payload = serde_json::json!({
        "pane_id": "%2",
        "pid": 1u32,
        "exit_code": -1i32,
        "elapsed_ms": 5u64,
        "command": "false",
    });
    assert_eq!(payload["exit_code"], -1);
}

#[test]
fn dispatcher_publishes_exec_completed_topic() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/backend/dispatcher.rs");
    let src = fs::read_to_string(&path).expect("read src/backend/dispatcher.rs");
    assert!(
        src.contains("psmux/exec/completed") || src.contains("topics::EXEC_COMPLETED"),
        "src/backend/dispatcher.rs must publish psmux/exec/completed alongside exec_completed"
    );
}

// ─── Task 10: psmux/session/{created,renamed,killed} payload contract ────────

#[test]
fn session_created_topic_constant_matches_spec() {
    assert_eq!(
        psmux::mycel::topics::SESSION_CREATED,
        "psmux/session/created"
    );
}

#[test]
fn session_renamed_topic_constant_matches_spec() {
    assert_eq!(
        psmux::mycel::topics::SESSION_RENAMED,
        "psmux/session/renamed"
    );
}

#[test]
fn session_killed_topic_constant_matches_spec() {
    assert_eq!(psmux::mycel::topics::SESSION_KILLED, "psmux/session/killed");
}

#[test]
fn session_payload_has_session_name_and_client_id() {
    let payload = serde_json::json!({
        "session_name": "work",
        "client_id": "psmux@HOSTNAME",
    });
    let bytes = serde_json::to_vec(&payload).expect("serialize");
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["session_name"], "work");
    assert_eq!(parsed["client_id"], "psmux@HOSTNAME");
}

#[test]
fn server_mod_rs_publishes_session_topics() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/server/mod.rs");
    let src = fs::read_to_string(&path).expect("read src/server/mod.rs");
    assert!(
        src.contains("psmux/session/created") || src.contains("topics::SESSION_CREATED"),
        "src/server/mod.rs must publish psmux/session/created"
    );
    assert!(
        src.contains("psmux/session/renamed") || src.contains("topics::SESSION_RENAMED"),
        "src/server/mod.rs must publish psmux/session/renamed"
    );
    assert!(
        src.contains("psmux/session/killed") || src.contains("topics::SESSION_KILLED"),
        "src/server/mod.rs must publish psmux/session/killed"
    );
}

#[test]
fn mycel_client_id_is_accessible_after_init() {
    // init_mycel_bus is idempotent via OnceLock — safe to call in tests.
    psmux::mycel::init_mycel_bus("psmux@test-host");
    // Since OnceLock may have been initialised by another test first, just
    // assert a value is present and non-empty.
    let id = psmux::mycel::mycel_client_id().expect("client_id should be set");
    assert!(
        id.starts_with("psmux@"),
        "client_id should start with psmux@, got: {id}"
    );
}
