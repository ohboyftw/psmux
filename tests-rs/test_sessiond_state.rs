use super::*;

fn state_line(session: &str, state: &str, reason: &str) -> String {
    format!(
        r#"{{"topic":"sessiond/session/state","published_at":"2026-10-21T20:41:07.901Z","payload":{{"session":"{session}","state":"{state}","since":"2026-10-21T20:41:07.512Z","reason":"{reason}"}}}}"#
    )
}

fn removed_line(session: &str) -> String {
    format!(
        r#"{{"topic":"sessiond/session/removed","payload":{{"session":"{session}","since":"2026-10-21T20:45:00.004Z","reason":"archived"}}}}"#
    )
}

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
fn test_apply_line_when_json_is_malformed_then_line_is_rejected() {
    let mut board = Board::default();
    assert!(!board.apply_line("{\"topic\": \"sessiond/session/state\""));
    assert_eq!(board.len(), 0);
}

#[test]
fn test_apply_line_when_topic_is_not_sessiond_then_line_is_rejected() {
    let mut board = Board::default();
    let line = r#"{"topic":"psmux/pane/exited","payload":{"session":"s1","state":"done"}}"#;
    assert!(!board.apply_line(line));
}

#[test]
fn test_apply_line_when_state_is_outside_v1_then_line_is_rejected() {
    let mut board = Board::default();
    assert!(!board.apply_line(&state_line("s1", "sleeping", "status:x")));
    assert_eq!(board.len(), 0);
}

#[test]
fn test_apply_line_when_payload_has_unknown_keys_then_they_are_ignored() {
    let line = r#"{"topic":"sessiond/session/state","payload":{"session":"s1","state":"idle","since":"t","reason":"status:idle","title":"new key"}}"#;
    let mut board = Board::default();
    assert!(board.apply_line(line));
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
