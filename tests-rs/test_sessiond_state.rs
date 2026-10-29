use super::*;

fn state_line(session: &str, state: &str, reason: &str) -> String {
    format!(
        r#"{{"topic":"sessiond/session/state","published_at":"2026-10-21T20:41:07.901Z","payload":{{"session":"{session}","state":"{state}","since":"2026-10-21T20:41:07.512Z","reason":"{reason}"}}}}"#
    )
}

fn titled_line(session: &str, state: &str, title: &str, project: &str) -> String {
    format!(
        r#"{{"topic":"sessiond/session/state","payload":{{"session":"{session}","state":"{state}","since":"t","reason":"r","title":"{title}","project":"{project}"}}}}"#
    )
}

fn removed_line(session: &str) -> String {
    format!(
        r#"{{"topic":"sessiond/session/removed","payload":{{"session":"{session}","since":"2026-10-21T20:45:00.004Z","reason":"archived"}}}}"#
    )
}

const STARTED_LINE: &str = r#"{"topic":"sessiond/sidecar/started","payload":{"at":"2026-10-22T00:30:00.000Z","pid":4242}}"#;
const ALIVE_LINE: &str = r#"{"topic":"sessiond/sidecar/alive","payload":{"at":"2026-10-22T00:30:30.000Z","started":"2026-10-22T00:30:00.000Z","sessions":2}}"#;

fn board_of(lines: &[String]) -> Board {
    let mut board = Board::default();
    for line in lines {
        board.apply_line(line);
    }
    board
}

#[test]
fn test_apply_line_when_state_message_then_session_is_on_board() {
    let board = board_of(&[state_line("s1", "blocked", "pending:command")]);
    let entry = board.get("s1").expect("s1 on the board");
    assert_eq!(entry.state, "blocked");
    assert_eq!(entry.since, "2026-10-21T20:41:07.512Z");
    assert_eq!(entry.reason, "pending:command");
}

#[test]
fn test_apply_line_when_state_has_title_and_project_then_both_are_kept() {
    let board = board_of(&[titled_line("s1", "done", "fix the bug", "psmux")]);
    let entry = board.get("s1").expect("s1 on the board");
    assert_eq!(entry.title.as_deref(), Some("fix the bug"));
    assert_eq!(entry.project.as_deref(), Some("psmux"));
}

#[test]
fn test_apply_line_when_session_changes_state_then_latest_state_wins() {
    let board = board_of(&[
        state_line("s1", "working", "activity:running"),
        state_line("s1", "done", "status:completed"),
    ]);
    assert_eq!(board.get("s1").map(|e| e.state.as_str()), Some("done"));
    assert_eq!(board.len(), 1);
}

#[test]
fn test_apply_line_when_removed_message_then_session_is_dropped() {
    let board = board_of(&[
        state_line("s1", "done", "status:completed"),
        removed_line("s1"),
    ]);
    assert_eq!(board.get("s1"), None);
}

#[test]
fn test_apply_line_when_sidecar_starts_then_board_is_cleared() {
    let mut board = board_of(&[state_line("s1", "done", "status:completed")]);
    assert_eq!(board.apply_line(STARTED_LINE), Applied::Started);
    assert_eq!(board.len(), 0);
}

#[test]
fn test_apply_line_when_heartbeat_then_board_is_unchanged() {
    let mut board = board_of(&[state_line("s1", "done", "status:completed")]);
    assert_eq!(board.apply_line(ALIVE_LINE), Applied::Alive);
    assert_eq!(board.len(), 1);
}

#[test]
fn test_apply_line_when_json_is_malformed_then_line_is_ignored() {
    let mut board = Board::default();
    let outcome = board.apply_line("{\"topic\": \"sessiond/session/state\"");
    assert_eq!(outcome, Applied::Ignored);
}

#[test]
fn test_apply_line_when_topic_is_not_sessiond_then_line_is_ignored() {
    let mut board = Board::default();
    let line = r#"{"topic":"psmux/pane/exited","payload":{"session":"s1","state":"done"}}"#;
    assert_eq!(board.apply_line(line), Applied::Ignored);
}

#[test]
fn test_apply_line_when_state_is_outside_v1_then_line_is_ignored() {
    let mut board = Board::default();
    let outcome = board.apply_line(&state_line("s1", "sleeping", "status:x"));
    assert_eq!(outcome, Applied::Ignored);
    assert_eq!(board.len(), 0);
}

#[test]
fn test_apply_line_when_payload_has_unknown_keys_then_they_are_ignored() {
    let line = r#"{"topic":"sessiond/session/state","payload":{"session":"s1","state":"idle","since":"t","reason":"status:idle","colour":"new key"}}"#;
    let mut board = Board::default();
    assert_eq!(board.apply_line(line), Applied::State);
}

#[test]
fn test_summary_when_board_is_empty_then_it_is_empty() {
    assert_eq!(Board::default().summary(), "");
}

#[test]
fn test_summary_when_states_are_mixed_then_most_urgent_comes_first() {
    let board = board_of(&[
        state_line("s1", "done", "status:completed"),
        state_line("s2", "working", "status:running"),
        state_line("s3", "blocked", "pending:user_input"),
        state_line("s4", "done", "status:completed"),
    ]);
    assert_eq!(board.summary(), "B1 W1 D2");
}

#[test]
fn test_attention_when_sessions_are_blocked_or_failed_then_only_they_are_listed() {
    let board = board_of(&[
        titled_line("s1", "error", "deploy", "layax"),
        titled_line("s2", "working", "refactor", "psmux"),
        titled_line("s3", "blocked", "fix the bug", "psmux"),
    ]);
    assert_eq!(board.attention(), "psmux: fix the bug | layax: deploy");
}

#[test]
fn test_attention_when_title_is_long_then_it_is_cut_with_an_ellipsis() {
    let title = "Read ~/overnight/T3-ORCHESTRATOR.md and do what it says";
    let board = board_of(&[titled_line("s1", "blocked", title, "psmux")]);
    assert_eq!(board.attention(), "psmux: Read ~/overnight/T3-ORCHESTRATO…");
}

#[test]
fn test_sessions_when_title_is_missing_then_the_session_id_is_shown() {
    let board = board_of(&[state_line("3f6c2a0e", "done", "status:completed")]);
    assert_eq!(board.sessions(), "D 3f6c2a0e");
}

#[test]
fn test_sessions_when_several_then_one_line_each_most_urgent_first() {
    let board = board_of(&[
        titled_line("s1", "done", "write docs", "psmux"),
        titled_line("s2", "blocked", "fix the bug", "psmux"),
    ]);
    assert_eq!(
        board.sessions(),
        "B psmux: fix the bug\nD psmux: write docs"
    );
}

#[test]
fn test_feed_status_when_command_is_not_running_then_down() {
    assert_eq!(feed_status(false, Duration::ZERO), "down");
}

#[test]
fn test_feed_status_when_sidecar_is_silent_past_the_limit_then_stale() {
    assert_eq!(feed_status(true, Duration::from_secs(91)), "stale");
}

#[test]
fn test_feed_status_when_sidecar_was_heard_recently_then_live() {
    assert_eq!(feed_status(true, Duration::from_secs(30)), "live");
}

#[test]
fn test_format_var_when_feed_option_is_unset_then_feed_reads_off() {
    let options = HashMap::new();
    assert_eq!(
        format_var("sessiond_feed", &options).as_deref(),
        Some("off")
    );
}

#[test]
fn test_format_var_when_feed_option_is_unset_then_counts_are_empty() {
    let options = HashMap::new();
    assert_eq!(
        format_var("sessiond_blocked", &options).as_deref(),
        Some("")
    );
}

#[test]
fn test_format_var_when_variable_is_not_sessiond_then_none() {
    assert_eq!(format_var("session_name", &HashMap::new()), None);
}
