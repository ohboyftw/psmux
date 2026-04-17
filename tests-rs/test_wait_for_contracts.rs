//! Contract tests for the `wait_for` module (Phase 2, Feature 4).
//!
//! These tests validate the `WaitCondition` parsing and `WaitOutcome`
//! serialization contracts that CLI, JSON-RPC, and orchestrate depend on.

use psmux::wait_for::{WaitCondition, WaitOutcome};

// ─── WaitCondition parsing ──────────────────────────────────────────────────

#[test]
fn parse_exit_condition_with_valid_pid() {
    let c = WaitCondition::parse("exit", Some("1234"), None).unwrap();
    assert!(matches!(c, WaitCondition::Exit { pid: 1234 }));
}

#[test]
fn parse_exit_condition_rejects_missing_pid() {
    assert!(WaitCondition::parse("exit", None, None).is_err());
}

#[test]
fn parse_exit_condition_rejects_non_numeric_pid() {
    assert!(WaitCondition::parse("exit", Some("abc"), None).is_err());
}

#[test]
fn parse_file_condition_with_valid_path() {
    let c = WaitCondition::parse("file", Some("./done.flag"), None).unwrap();
    match c {
        WaitCondition::File { ref path } => {
            assert_eq!(path.to_str().unwrap(), "./done.flag");
        }
        _ => panic!("expected File variant"),
    }
}

#[test]
fn parse_file_condition_rejects_missing_path() {
    assert!(WaitCondition::parse("file", None, None).is_err());
}

#[test]
fn parse_output_condition_with_valid_regex() {
    let c = WaitCondition::parse("output", Some("^PROMPT_READY$"), None).unwrap();
    assert!(matches!(c, WaitCondition::Output { .. }));
}

#[test]
fn parse_output_condition_rejects_invalid_regex() {
    assert!(WaitCondition::parse("output", Some("[invalid"), None).is_err());
}

#[test]
fn parse_output_condition_rejects_missing_pattern() {
    assert!(WaitCondition::parse("output", None, None).is_err());
}

#[test]
fn parse_ready_condition() {
    let c = WaitCondition::parse("ready", None, None).unwrap();
    assert!(matches!(c, WaitCondition::Ready));
}

#[test]
fn parse_unknown_kind_returns_error() {
    assert!(WaitCondition::parse("bogus", None, None).is_err());
}

// ─── WaitOutcome serialization ──────────────────────────────────────────────

#[test]
fn outcome_success_serializes_with_elapsed() {
    let o = WaitOutcome::Success { elapsed_ms: 42 };
    let j = serde_json::to_string(&o).unwrap();
    assert!(j.contains("\"kind\":\"success\""));
    assert!(j.contains("\"elapsed_ms\":42"));
}

#[test]
fn outcome_timeout_serializes_with_elapsed() {
    let o = WaitOutcome::Timeout { elapsed_ms: 5000 };
    let j = serde_json::to_string(&o).unwrap();
    assert!(j.contains("\"kind\":\"timeout\""));
    assert!(j.contains("\"elapsed_ms\":5000"));
}

#[test]
fn outcome_error_serializes_with_reason() {
    let o = WaitOutcome::Error {
        reason: "process not found".into(),
    };
    let j = serde_json::to_string(&o).unwrap();
    assert!(j.contains("\"kind\":\"error\""));
    assert!(j.contains("process not found"));
}

#[test]
fn outcome_success_deserializes_roundtrip() {
    let o = WaitOutcome::Success { elapsed_ms: 99 };
    let j = serde_json::to_string(&o).unwrap();
    let o2: WaitOutcome = serde_json::from_str(&j).unwrap();
    assert!(matches!(o2, WaitOutcome::Success { elapsed_ms: 99 }));
}

#[test]
fn outcome_exit_success_carries_exit_code() {
    let o = WaitOutcome::ExitSuccess {
        elapsed_ms: 200,
        exit_code: 0,
    };
    let j = serde_json::to_string(&o).unwrap();
    assert!(j.contains("\"exit_code\":0"));
    let o2: WaitOutcome = serde_json::from_str(&j).unwrap();
    assert!(matches!(
        o2,
        WaitOutcome::ExitSuccess {
            elapsed_ms: 200,
            exit_code: 0,
        }
    ));
}

// ─── WaitCondition display ──────────────────────────────────────────────────

#[test]
fn exit_condition_displays_pid() {
    let c = WaitCondition::parse("exit", Some("42"), None).unwrap();
    let s = format!("{c}");
    assert!(s.contains("42"));
}

#[test]
fn file_condition_displays_path() {
    let c = WaitCondition::parse("file", Some("sentinel.txt"), None).unwrap();
    let s = format!("{c}");
    assert!(s.contains("sentinel.txt"));
}

#[test]
fn output_condition_displays_pattern() {
    let c = WaitCondition::parse("output", Some("hello.*world"), None).unwrap();
    let s = format!("{c}");
    assert!(s.contains("hello.*world"));
}

#[test]
fn ready_condition_displays() {
    let c = WaitCondition::parse("ready", None, None).unwrap();
    let s = format!("{c}");
    assert!(s.contains("ready"));
}
