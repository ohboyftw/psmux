use chrono::{TimeZone, Utc};
use psmux_fleet::{AgentKind, Workspace, WorkspaceStatus};
use std::collections::HashMap;
use std::path::PathBuf;

#[test]
fn workspace_round_trips_json() {
    let w = Workspace {
        id: "fix-auth-bug".into(),
        branch: Some("fleet/fix-auth-bug".into()),
        worktree_path: Some(PathBuf::from("/tmp/fix-auth-bug")),
        pane_id: Some("%3".into()),
        session: "demo".into(),
        agent_kind: AgentKind::Claude,
        status: WorkspaceStatus::Running,
        task: Some("fix the auth bug".into()),
        started_at: Some(Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap()),
        finished_at: None,
        exit_code: None,
        crash_dump_path: None,
        metadata: HashMap::new(),
    };
    let json = serde_json::to_string(&w).unwrap();
    let back: Workspace = serde_json::from_str(&json).unwrap();
    assert_eq!(back, w);
}

#[test]
fn agent_kind_unknown_for_untagged_pane() {
    assert_eq!(AgentKind::default(), AgentKind::Unknown);
}

#[test]
fn workspace_status_serializes_snake_case() {
    assert_eq!(
        serde_json::to_string(&WorkspaceStatus::Running).unwrap(),
        r#""running""#
    );
    assert_eq!(
        serde_json::to_string(&WorkspaceStatus::Awaiting).unwrap(),
        r#""awaiting""#
    );
    assert_eq!(
        serde_json::to_string(&WorkspaceStatus::Succeeded).unwrap(),
        r#""succeeded""#
    );
}

#[test]
fn workspace_deserializes_without_metadata_field() {
    let json = r#"{"id":"x","session":"s","agent_kind":"shell","status":"pending"}"#;
    let w: Workspace = serde_json::from_str(json).unwrap();
    assert!(w.metadata.is_empty());
}

#[test]
fn workspace_status_deserializes_from_snake_case() {
    let s: WorkspaceStatus = serde_json::from_str(r#""running""#).unwrap();
    assert_eq!(s, WorkspaceStatus::Running);
    let s: WorkspaceStatus = serde_json::from_str(r#""awaiting""#).unwrap();
    assert_eq!(s, WorkspaceStatus::Awaiting);
}
