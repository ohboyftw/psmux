use super::*;

fn report(parser: &mut vt100::Parser, body: &str) {
    parser.process(format!("\x1b]7501;{body}\x1b\\").as_bytes());
}

fn label(cache: &AgentStateCache) -> &'static str {
    state_label(cache.worst())
}

#[test]
fn observe_when_first_report_then_transition_from_none() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut parser, "state=working");
    let t = cache.observe(parser.screen()).expect("transition");
    assert_eq!(t.old, None);
    assert_eq!(t.new, Some(ProgramState::Working));
    assert_eq!(t.record_id, None);
}

#[test]
fn observe_when_progress_only_changes_then_no_transition() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut parser, "state=working:progress=10");
    cache.observe(parser.screen());
    report(&mut parser, "state=working:progress=50");
    assert_eq!(cache.observe(parser.screen()), None);
}

#[test]
fn observe_when_nothing_changed_then_no_transition() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    assert_eq!(cache.observe(parser.screen()), None);
    report(&mut parser, "state=idle");
    cache.observe(parser.screen());
    assert_eq!(cache.observe(parser.screen()), None);
}

#[test]
fn observe_when_child_blocked_then_transition_names_child() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut parser, "state=working");
    cache.observe(parser.screen());
    report(&mut parser, "id=t1:state=blocked:kind=question");
    let t = cache.observe(parser.screen()).expect("transition");
    assert_eq!(t.old, Some(ProgramState::Working));
    assert_eq!(t.new, Some(ProgramState::Blocked));
    assert_eq!(t.record_id.as_deref(), Some("t1"));
    assert_eq!(cache.root(), Some(ProgramState::Working));
}

#[test]
fn worst_when_error_and_done_children_then_error() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut parser, "state=idle");
    report(&mut parser, "id=a:state=done");
    report(&mut parser, "id=b:state=error");
    cache.observe(parser.screen());
    assert_eq!(label(&cache), "error");
}

#[test]
fn observe_when_new_parser_seq_climbs_back_then_state_refreshes() {
    let mut old = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut old, "state=working");
    report(&mut old, "state=working");
    cache.observe(old.screen());
    let mut new = vt100::Parser::new(24, 80, 0);
    cache.parser_replaced();
    report(&mut new, "state=idle");
    report(&mut new, "state=idle");
    assert_eq!(
        new.screen().program_status_seq(),
        old.screen().program_status_seq()
    );
    let t = cache.observe(new.screen()).expect("transition");
    assert_eq!(t.new, Some(ProgramState::Idle));
}

#[test]
fn parser_replaced_when_working_then_empty_and_transition_to_none() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut parser, "state=working");
    cache.observe(parser.screen());
    cache.parser_replaced();
    assert_eq!(label(&cache), "");
    let fresh = vt100::Parser::new(24, 80, 0);
    let t = cache.observe(fresh.screen()).expect("transition");
    assert_eq!(t.old, Some(ProgramState::Working));
    assert_eq!(t.new, None);
    assert_eq!(label(&cache), "");
}

fn exit_and_refresh(parser: &mut vt100::Parser, cache: &mut AgentStateCache) {
    parser.screen_mut().program_status_on_process_exit();
    cache.refresh(parser.screen());
}

#[test]
fn refresh_when_process_exits_while_working_then_empty() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut parser, "state=working:msg=YWI=");
    cache.observe(parser.screen());
    exit_and_refresh(&mut parser, &mut cache);
    assert_eq!(label(&cache), "");
    assert_eq!(cache.details(), &AgentRecordDetails::default());
}

#[test]
fn refresh_when_process_exits_while_blocked_then_empty() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut parser, "state=blocked:kind=permission");
    cache.observe(parser.screen());
    exit_and_refresh(&mut parser, &mut cache);
    assert_eq!(label(&cache), "");
}

#[test]
fn refresh_when_process_exits_with_done_child_then_done_survives() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut parser, "state=working");
    report(&mut parser, "id=t1:state=done");
    cache.observe(parser.screen());
    exit_and_refresh(&mut parser, &mut cache);
    assert_eq!(label(&cache), "done");
}

#[test]
fn observe_after_exit_refresh_then_final_transition_still_reported() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut parser, "state=working");
    cache.observe(parser.screen());
    exit_and_refresh(&mut parser, &mut cache);
    let t = cache.observe(parser.screen()).expect("transition");
    assert_eq!((t.old, t.new), (Some(ProgramState::Working), None));
}

/// Shape Claude Code 2.1.295 emits (tasklist section 0): root plus one child
/// per background task.  Synthetic until a live capture replaces it.
const CC_FIXTURE: &[&str] = &[
    "state=working:app=claude-code:progress=10:title=UGxhbiByZWFkeQ==",
    "state=blocked:id=t1:kind=permission:progress=40:title=UGxhbiByZWFkeQ==:msg=QXBwcm92ZSBwbGFuPw==",
];

#[test]
fn details_when_cc_child_blocked_then_every_key_decoded() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    CC_FIXTURE.iter().for_each(|body| report(&mut parser, body));
    let t = cache.observe(parser.screen()).expect("transition");
    assert_eq!(t.record_id.as_deref(), Some("t1"));
    let expected = AgentRecordDetails {
        kind: Some(vt100::BlockedKind::Permission),
        progress: Some(40),
        app: "claude-code".into(),
        title: "Plan ready".into(),
        msg: "Approve plan?".into(),
    };
    assert_eq!(cache.details(), &expected);
}

#[test]
fn details_when_cc_root_only_then_root_keys_decoded() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut parser, CC_FIXTURE[0]);
    cache.observe(parser.screen());
    let d = cache.details();
    assert_eq!(
        (
            d.kind,
            d.progress,
            d.app.as_str(),
            d.title.as_str(),
            d.msg.as_str()
        ),
        (None, Some(10), "claude-code", "Plan ready", "")
    );
}

#[test]
fn details_when_msg_has_rtl_override_then_override_removed() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    // "a\u{202E}b"
    report(&mut parser, "state=blocked:msg=YeKArmI=");
    cache.observe(parser.screen());
    assert_eq!(cache.details().msg, "ab");
}

#[test]
fn details_when_msg_longer_than_cap_then_truncated() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    let long = "eHh4".repeat(100); // "xxx" x 100
    report(&mut parser, &format!("state=working:msg={long}"));
    cache.observe(parser.screen());
    assert_eq!(cache.details().msg, "x".repeat(MAX_TEXT_CHARS));
}

#[test]
fn sanitize_text_when_hidden_format_chars_then_all_removed() {
    let hidden = "\u{200B}\u{200F}\u{202A}\u{202E}\u{2066}\u{2069}\u{FEFF}";
    assert_eq!(sanitize_text(&format!("o{hidden}k")), "ok");
}

#[test]
fn parser_replaced_when_details_cached_then_details_empty() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut parser, CC_FIXTURE[1]);
    cache.observe(parser.screen());
    cache.parser_replaced();
    assert_eq!(cache.details(), &AgentRecordDetails::default());
}

/// Orchestrate splits `list-panes -F` output on `:`, so only the state
/// label (a closed set) may ever be appended there, never msg/title/app.
#[test]
fn state_label_when_any_state_then_closed_set_without_colon() {
    let states = [
        None,
        Some(ProgramState::Idle),
        Some(ProgramState::Working),
        Some(ProgramState::Done),
        Some(ProgramState::Blocked),
        Some(ProgramState::Error),
    ];
    for s in states {
        assert!(!state_label(s).contains(':'));
    }
}
