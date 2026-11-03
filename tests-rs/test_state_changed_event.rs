//! `state_changed` push built from an agent-state transition (ticket 04a).

use super::*;
use crate::agent_state::{AgentRecordDetails, AgentTransition};
use vt100::{BlockedKind, ProgramState};

fn to_json(t: &AgentTransition) -> serde_json::Value {
    serde_json::to_value(StateChangedEvent::new(7, t)).expect("event must serialize")
}

#[test]
fn state_changed_when_child_blocked_then_carries_kind_app_and_record_id() {
    let json = to_json(&AgentTransition {
        old: Some(ProgramState::Working),
        new: Some(ProgramState::Blocked),
        record_id: Some("t1".into()),
        details: AgentRecordDetails {
            kind: Some(BlockedKind::Question),
            app: "claude-code".into(),
            msg: "not sent".into(),
            ..AgentRecordDetails::default()
        },
    });
    assert_eq!(json["method"], "state_changed");
    let p = &json["params"];
    assert_eq!(p["context_id"], "%7");
    assert_eq!(p["state"], "blocked");
    assert_eq!(p["previous_state"], "working");
    assert_eq!(p["record_id"], "t1");
    assert_eq!(p["kind"], "question");
    assert_eq!(p["app"], "claude-code");
    assert!(p.get("msg").is_none(), "msg and title stay out of the push");
}

#[test]
fn state_changed_when_record_removed_then_state_is_null_and_extras_absent() {
    let json = to_json(&AgentTransition {
        old: Some(ProgramState::Working),
        new: None,
        record_id: None,
        details: AgentRecordDetails::default(),
    });
    let p = json["params"].as_object().expect("params object");
    assert!(p["state"].is_null());
    assert_eq!(p["previous_state"], "working");
    for key in ["record_id", "kind", "app"] {
        assert!(!p.contains_key(key), "{key} must be omitted when empty");
    }
}

#[test]
fn state_changed_when_first_report_then_previous_state_is_null() {
    let json = to_json(&AgentTransition {
        old: None,
        new: Some(ProgramState::Idle),
        record_id: None,
        details: AgentRecordDetails::default(),
    });
    assert!(json["params"]["previous_state"].is_null());
    assert_eq!(json["params"]["state"], "idle");
}
