//! Ticket 08: orchestrate reads `#{pane_agent_state}` but never lets it end a
//! worker. Included from `src/orchestrate.rs`.

use super::*;

fn running(agent_state: Option<&str>) -> WorkerState {
    let mut ws = WorkerState::new_pending();
    ws.status = WorkerStatus::Running;
    ws.agent_state = agent_state.map(str::to_string);
    ws
}

fn state_with(id: &str, ws: WorkerState) -> OrchestrationState {
    OrchestrationState {
        session: "s".into(),
        workers: HashMap::from([(id.to_string(), ws)]),
    }
}

#[test]
fn parse_when_alive_with_state_then_alive_carries_it() {
    let raw = "%1:0::working\n%2:0::blocked\n";
    let check = parse_pane_check(raw, "%2");
    assert_eq!(
        check,
        PaneCheck::Alive {
            agent_state: Some("blocked".into())
        }
    );
}

#[test]
fn parse_when_state_field_empty_then_none() {
    assert_eq!(
        parse_pane_check("%4:0::\n", "%4"),
        PaneCheck::Alive { agent_state: None }
    );
}

#[test]
fn parse_when_old_three_field_line_then_none() {
    assert_eq!(
        parse_pane_check("%4:0:\n", "%4"),
        PaneCheck::Alive { agent_state: None }
    );
}

#[test]
fn parse_when_dead_with_done_then_code_and_done() {
    assert_eq!(
        parse_pane_check("%3:1:0:done\n", "%3"),
        PaneCheck::Dead {
            code: Some(0),
            agent_state: Some("done".into())
        }
    );
}

#[test]
fn parse_when_dead_while_blocked_then_dead_wins() {
    assert_eq!(
        parse_pane_check("%3:1:7:blocked\n", "%3"),
        PaneCheck::Dead {
            code: Some(7),
            agent_state: Some("blocked".into())
        }
    );
}

#[test]
fn parse_when_dead_without_exit_code_then_unknown_code_and_state_kept() {
    assert_eq!(
        parse_pane_check("%3:1::done\n", "%3"),
        PaneCheck::Dead {
            code: None,
            agent_state: Some("done".into())
        }
    );
}

#[test]
fn record_death_when_exit_code_unknown_then_failed_with_state_kept() {
    let mut ws = running(Some("working"));
    record_pane_death(&mut ws, None, Some("done".into()));
    assert_eq!(ws.status, WorkerStatus::Failed);
    assert_eq!(ws.exit_code, None);
    assert_eq!(ws.agent_state.as_deref(), Some("done"));
    assert_eq!(ws.crash_dump_path, None);
}

#[test]
fn record_death_when_exit_code_unknown_then_pane_is_killed() {
    let mut ws = running(None);
    assert!(record_pane_death(&mut ws, None, None));
}

#[test]
fn record_death_when_exit_zero_then_succeeded_and_pane_is_killed() {
    let mut ws = running(Some("working"));
    assert!(record_pane_death(&mut ws, Some(0), Some("done".into())));
    assert_eq!(ws.status, WorkerStatus::Succeeded);
    assert_eq!(ws.exit_code, Some(0));
    assert_eq!(ws.agent_state.as_deref(), Some("done"));
}

#[test]
fn record_death_when_session_gone_then_last_state_kept_and_nothing_to_kill() {
    let mut ws = running(Some("blocked"));
    assert!(!record_pane_death(&mut ws, Some(EXIT_SESSION_GONE), None));
    assert_eq!(ws.status, WorkerStatus::Failed);
    assert_eq!(ws.exit_code, Some(EXIT_SESSION_GONE));
    assert_eq!(ws.agent_state.as_deref(), Some("blocked"));
}

#[test]
fn parse_when_pane_absent_then_pane_gone() {
    assert_eq!(
        parse_pane_check("%9:0::working\n", "%3"),
        PaneCheck::Dead {
            code: Some(EXIT_PANE_GONE),
            agent_state: None
        }
    );
}

#[test]
fn blocked_running_worker_when_checked_then_not_terminal() {
    let state = state_with("a", running(Some("blocked")));
    assert!(!state.workers["a"].is_terminal());
    assert!(state.completed_ids().is_empty());
}

#[test]
fn record_when_state_unchanged_then_reports_no_change() {
    let mut ws = running(Some("working"));
    assert!(!record_agent_state(&mut ws, Some("working".into())));
    assert!(record_agent_state(&mut ws, Some("blocked".into())));
    assert_eq!(ws.agent_state.as_deref(), Some("blocked"));
}

#[test]
fn timeout_when_worker_blocked_then_failed_and_named() {
    let mut state = state_with("a", running(Some("blocked")));
    state.workers.insert("b".into(), running(Some("working")));
    let blocked = fail_unfinished_on_timeout(&mut state);
    assert_eq!(blocked, vec!["a".to_string()]);
    let a = &state.workers["a"];
    assert_eq!(a.status, WorkerStatus::Failed);
    assert_eq!(a.exit_code, Some(-2));
    assert_eq!(a.agent_state.as_deref(), Some("blocked"));
}

#[test]
fn timeout_message_when_blocked_then_keeps_prefix_and_lists_ids() {
    let msg = timeout_message(500, &["a".into(), "c".into()]);
    assert!(msg.starts_with("orchestrate timed out"));
    assert!(msg.ends_with("; blocked: a, c"));
    assert_eq!(
        timeout_message(500, &[]),
        "orchestrate timed out after 500 ms"
    );
}

#[test]
fn load_when_state_json_predates_agent_state_then_none() {
    let json = r#"{"session":"s","workers":{"a":{"status":"running","pane_id":"%1"}}}"#;
    let state: OrchestrationState = serde_json::from_str(json).unwrap();
    assert_eq!(state.workers["a"].agent_state, None);
}

#[test]
fn save_when_agent_state_none_then_key_absent() {
    let json = serde_json::to_string(&running(None)).unwrap();
    assert!(!json.contains("agent_state"));
}
