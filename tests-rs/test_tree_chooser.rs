use super::*;
use crate::util::PaneInfo;

fn pane(id: usize, agent: &str) -> PaneInfo {
    PaneInfo {
        id,
        title: format!("p{id}"),
        agent_state: agent.to_string(),
    }
}

fn win(id: usize, panes: Vec<PaneInfo>) -> WinTree {
    WinTree {
        id,
        name: format!("w{id}"),
        active: id == 0,
        panes,
    }
}

fn agents(rows: &[TreeRow]) -> Vec<(&str, &str)> {
    rows.iter().map(|r| (r.label.as_str(), r.agent)).collect()
}

#[test]
fn build_rows_when_pane_blocked_then_pane_window_and_header_marked() {
    let sessions = vec![(
        "work".to_string(),
        vec![
            win(0, vec![pane(1, "working"), pane(2, "blocked")]),
            win(1, vec![pane(3, "")]),
        ],
    )];
    let rows = build_rows(&sessions, "work");
    assert_eq!(
        agents(&rows),
        vec![
            ("work: 2 windows (attached)", "blocked"),
            ("  0: w0 (2 panes)", "blocked"),
            ("    p1", "working"),
            ("    p2", "blocked"),
            ("  1: w1 (1 panes)", ""),
            ("    p3", ""),
        ]
    );
}

#[test]
fn build_rows_when_other_session_blocked_then_collapsed_rows_still_marked() {
    let sessions = vec![
        ("me".to_string(), vec![win(0, vec![pane(1, "")])]),
        (
            "other".to_string(),
            vec![win(4, vec![pane(5, "done"), pane(6, "error")])],
        ),
    ];
    let rows = build_rows(&sessions, "me");
    let other: Vec<_> = rows.iter().filter(|r| r.session == "other").collect();
    assert_eq!(other.len(), 2, "other session shows header and window only");
    assert_eq!(other[0].agent, "error");
    assert_eq!(other[1].agent, "error");
    assert!(other[0].is_header());
}

#[test]
fn build_rows_when_nothing_reported_then_no_glyph_anywhere() {
    let sessions = vec![("s".to_string(), vec![win(0, vec![pane(1, "")])])];
    assert!(build_rows(&sessions, "s")
        .iter()
        .all(|r| r.agent.is_empty()));
    assert!(glyph_span("").is_none());
}

#[test]
fn glyph_span_when_blocked_then_bang() {
    let span = glyph_span("blocked").expect("glyph");
    assert_eq!(span.content, "!");
}

#[test]
fn list_tree_json_when_older_server_without_agent_state_then_parses() {
    let json = r#"[{"id":0,"name":"w","active":true,"panes":[{"id":1,"title":"t"}]}]"#;
    let wins: Vec<WinTree> = serde_json::from_str(json).expect("older server JSON parses");
    assert_eq!(wins[0].panes[0].agent_state, "");
}

#[test]
fn list_tree_json_when_state_set_then_round_trips() {
    let json = serde_json::to_string(&vec![win(0, vec![pane(1, "blocked")])]).expect("json");
    assert!(json.contains(r#""agent_state":"blocked""#));
    let wins: Vec<WinTree> = serde_json::from_str(&json).expect("parse");
    assert_eq!(wins[0].panes[0].agent_state, "blocked");
}

#[test]
fn list_tree_json_when_state_empty_then_field_omitted() {
    let json = serde_json::to_string(&vec![win(0, vec![pane(1, "")])]).expect("json");
    assert!(!json.contains("agent_state"));
}

#[test]
fn fallback_rows_when_pane_blocked_then_no_glyph_from_stale_tree() {
    let rows = fallback_rows(&[win(0, vec![pane(1, "blocked"), pane(2, "idle")])], "s");
    assert_eq!(agents(&rows), vec![("w0", ""), ("p1", ""), ("p2", "")]);
}

fn line_text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[test]
fn row_line_when_label_long_then_glyph_leads() {
    let mut r = row(false, 0, 1, "x".repeat(200), "s");
    r.agent = "blocked";
    let line = row_line(&r, Style::default());
    assert!(line_text(&line).starts_with("! x"));
}

#[test]
fn row_line_when_no_state_then_label_at_same_column() {
    let r = row(false, 0, 1, "label".to_string(), "s");
    assert_eq!(line_text(&row_line(&r, Style::default())), "  label");
}

#[test]
fn row_line_when_selected_then_glyph_keeps_row_background() {
    let mut r = row(false, 0, 1, "p".to_string(), "s");
    r.agent = "working";
    let sel = Style::default().bg(Color::Blue);
    let line = row_line(&r, sel);
    assert_eq!(line.spans[0].style.bg, Some(Color::Blue));
    assert_eq!(line.spans[0].style.fg, Some(Color::Yellow));
}

#[test]
fn with_agent_glyph_when_state_then_glyph_before_label() {
    let line = with_agent_glyph(Span::raw("window"), "error");
    assert_eq!(line_text(&line), "✗ window");
}

fn poll(listed: &[&str], answered: Vec<SessionTree>) -> SessionPoll {
    SessionPoll {
        listed: listed.iter().map(|s| s.to_string()).collect(),
        answered,
    }
}

fn tree(name: &str, agent: &str) -> SessionTree {
    (name.to_string(), vec![win(0, vec![pane(1, agent)])])
}

fn names(sessions: &[SessionTree]) -> Vec<&str> {
    sessions.iter().map(|(n, _)| n.as_str()).collect()
}

#[test]
fn merge_poll_when_listed_session_misses_then_previous_rows_kept() {
    let shown = vec![tree("me", "idle"), tree("other", "blocked")];
    let mut misses = HashMap::new();
    let merged = merge_poll(
        &shown,
        &mut misses,
        poll(&["me", "other"], vec![tree("other", "done")]),
        "me",
    );
    assert_eq!(merged, vec![tree("me", "idle"), tree("other", "done")]);
}

#[test]
fn merge_poll_when_session_misses_max_polls_then_dropped() {
    let shown = vec![tree("me", ""), tree("other", "")];
    let mut misses = HashMap::new();
    let mut merged = shown.clone();
    for _ in 1..MAX_MISSES {
        merged = merge_poll(
            &merged,
            &mut misses,
            poll(&["me", "other"], vec![tree("me", "")]),
            "me",
        );
        assert_eq!(names(&merged), vec!["me", "other"]);
    }
    merged = merge_poll(
        &merged,
        &mut misses,
        poll(&["me", "other"], vec![tree("me", "")]),
        "me",
    );
    assert_eq!(names(&merged), vec!["me"]);
}

#[test]
fn merge_poll_when_port_file_gone_then_dropped_at_once() {
    let shown = vec![tree("me", ""), tree("gone", "")];
    let mut misses = HashMap::new();
    let merged = merge_poll(&shown, &mut misses, poll(&["me"], vec![]), "me");
    assert_eq!(names(&merged), vec!["me"]);
}

#[test]
fn merge_poll_when_session_answers_again_then_miss_count_reset() {
    let shown = vec![tree("me", "")];
    let mut misses = HashMap::new();
    let none = || poll(&["me"], vec![]);
    let merged = merge_poll(&shown, &mut misses, none(), "me");
    let merged = merge_poll(
        &merged,
        &mut misses,
        poll(&["me"], vec![tree("me", "")]),
        "me",
    );
    assert!(misses.is_empty());
    let merged = merge_poll(&merged, &mut misses, none(), "me");
    assert_eq!(names(&merged), vec!["me"]);
}

#[test]
fn refreshed_rows_when_poll_empty_then_rows_kept() {
    let old = build_rows(&[tree("me", "")], "me");
    assert!(refreshed_rows(&old, 1, &[], "me").is_none());
}

#[test]
fn refreshed_rows_when_poll_has_rows_then_replaced_and_reselected() {
    let old = build_rows(&[tree("me", "")], "me");
    let sessions = vec![tree("me", "blocked"), tree("zz", "")];
    let (rows, selected) = refreshed_rows(&old, 2, &sessions, "me").expect("rows");
    assert_eq!(rows, build_rows(&sessions, "me"));
    assert_eq!(selected, 2);
    assert_eq!(rows[2].agent, "blocked");
}

#[test]
fn poller_when_poll_delivered_then_yields_once_and_waits_interval() {
    let (tx, rx) = mpsc::channel();
    let mut poller = TreePoller::default();
    poller.restart(vec![tree("me", "")]);
    poller.pending = Some(rx);
    tx.send(poll(&["me"], vec![tree("me", "working")]))
        .expect("send");
    assert_eq!(poller.poll(true, "me"), Some(vec![tree("me", "working")]));
    assert!(poller.poll(true, "me").is_none());
    assert!(poller.pending.is_none(), "next poll waits a full interval");
}

#[test]
fn rows_or_fallback_when_no_session_answered_then_last_tree() {
    let rows = rows_or_fallback(&[], "s", &[win(0, vec![pane(1, "")])]);
    assert_eq!(rows.len(), 2);
}

#[test]
fn reselect_when_row_still_present_then_follows_it() {
    let old = fallback_rows(&[win(0, vec![pane(1, ""), pane(2, "")])], "s");
    let new = fallback_rows(
        &[
            win(9, vec![pane(7, "")]),
            win(0, vec![pane(1, ""), pane(2, "")]),
        ],
        "s",
    );
    assert_eq!(reselect(&old, 2, &new), 4);
}

#[test]
fn reselect_when_row_gone_then_clamped() {
    let old = fallback_rows(&[win(0, vec![pane(1, ""), pane(2, "")])], "s");
    let new = fallback_rows(&[win(5, vec![])], "s");
    assert_eq!(reselect(&old, 2, &new), 0);
    assert_eq!(reselect(&old, 2, &[]), 0);
}

#[test]
fn poller_when_closed_then_never_polls() {
    let mut poller = TreePoller {
        last: Instant::now() - REPOLL_INTERVAL * 2,
        ..TreePoller::default()
    };
    assert!(poller.poll(false, "s").is_none());
    assert!(poller.pending.is_none());
}

#[test]
fn poller_when_interval_not_passed_then_no_poll_started() {
    let mut poller = TreePoller::default();
    assert!(poller.poll(true, "s").is_none());
    assert!(poller.pending.is_none());
}
