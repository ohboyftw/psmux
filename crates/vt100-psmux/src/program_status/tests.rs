use crate::{BlockedKind, Parser, ProgramState};

const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(input: &[u8]) -> String {
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let mut buf = [0u8; 3];
        buf[..chunk.len()].copy_from_slice(chunk);
        let n = (u32::from(buf[0]) << 16) | (u32::from(buf[1]) << 8) | u32::from(buf[2]);
        for i in 0..=chunk.len() {
            let idx = usize::try_from((n >> (18 - 6 * i)) & 63).unwrap();
            out.push(char::from(B64[idx]));
        }
        for _ in chunk.len()..3 {
            out.push('=');
        }
    }
    out
}

fn report(body: &str) -> Vec<u8> {
    format!("\x1b]7501;{body}\x1b\\").into_bytes()
}

fn parser_with(bodies: &[&str]) -> Parser {
    let mut parser = Parser::new(24, 80, 0);
    for body in bodies {
        parser.process(&report(body));
    }
    parser
}

fn root_state(parser: &Parser) -> Option<ProgramState> {
    parser
        .screen()
        .program_status()
        .map(crate::ProgramStatusRecord::state)
}

fn ids(parser: &Parser) -> Vec<Option<String>> {
    parser
        .screen()
        .program_status_records()
        .map(|(id, _)| id.map(str::to_owned))
        .collect()
}

/// Asserts that `body` is discarded and leaves the store untouched.
fn assert_rejected(body: &str) {
    let mut parser = parser_with(&["state=working:app=base"]);
    let before: Vec<_> = parser
        .screen()
        .program_status_records()
        .map(|(id, r)| (id.map(str::to_owned), r.clone()))
        .collect();
    let seq = parser.screen().program_status_seq();
    parser.process(&report(body));
    let after: Vec<_> = parser
        .screen()
        .program_status_records()
        .map(|(id, r)| (id.map(str::to_owned), r.clone()))
        .collect();
    assert_eq!(before, after, "report mutated records: {body:.80}");
    assert_eq!(
        seq,
        parser.screen().program_status_seq(),
        "seq bumped: {body:.80}"
    );
}

fn assert_accepted(body: &str) {
    let parser = parser_with(&[body]);
    assert!(
        parser.screen().program_status_seen(),
        "rejected: {body:.80}"
    );
}

#[test]
fn query_when_esc_terminated_then_replies_exact_bytes() {
    let mut parser = parser_with(&["?"]);
    assert_eq!(parser.screen_mut().take_replies(), b"\x1b]7501;?\x1b\\");
}

#[test]
fn query_when_bel_terminated_then_replies_exact_bytes() {
    let mut parser = Parser::new(24, 80, 0);
    parser.process(b"\x1b]7501;?\x07");
    assert_eq!(parser.screen_mut().take_replies(), b"\x1b]7501;?\x1b\\");
}

#[test]
fn query_when_answered_then_records_untouched() {
    let parser = parser_with(&["?"]);
    assert!(!parser.screen().program_status_seen());
    assert_eq!(parser.screen().program_status_seq(), 0);
}

#[test]
fn report_when_each_state_then_root_has_that_state() {
    for (name, state) in [
        ("idle", ProgramState::Idle),
        ("working", ProgramState::Working),
        ("done", ProgramState::Done),
        ("blocked", ProgramState::Blocked),
        ("error", ProgramState::Error),
    ] {
        let parser = parser_with(&[&format!("state={name}")]);
        assert_eq!(root_state(&parser), Some(state));
    }
}

#[test]
fn report_when_bel_terminated_then_applied() {
    let mut parser = Parser::new(24, 80, 0);
    parser.process(b"\x1b]7501;state=working\x07");
    assert_eq!(root_state(&parser), Some(ProgramState::Working));
}

#[test]
fn report_when_state_unknown_then_ignored() {
    let parser = parser_with(&["state=sleeping:app=pi"]);
    assert!(parser.screen().program_status().is_none());
    assert!(!parser.screen().program_status_seen());
}

#[test]
fn report_when_unknown_key_then_ignored_and_rest_applied() {
    let parser = parser_with(&["state=done:future=1:app=pi"]);
    assert_eq!(parser.screen().program_status().unwrap().app(), Some("pi"));
}

#[test]
fn report_when_record_exists_then_replaces_it_completely() {
    let msg = b64(b"hello");
    let parser = parser_with(&[
        &format!("state=working:app=pi:progress=5:msg={msg}"),
        "state=done",
    ]);
    let root = parser.screen().program_status().unwrap();
    assert_eq!(root.state(), ProgramState::Done);
    assert_eq!(
        (root.app(), root.progress(), root.msg()),
        (None, None, None)
    );
}

#[test]
fn clear_when_no_id_then_removes_every_record() {
    let parser = parser_with(&[
        "state=working",
        "state=working:id=a",
        "state=done:id=a/b",
        "state=clear",
    ]);
    assert!(ids(&parser).is_empty());
}

#[test]
fn clear_when_id_then_removes_record_and_descendants_only() {
    let parser = parser_with(&[
        "state=working",
        "state=working:id=a",
        "state=done:id=a/b/c",
        "state=done:id=ab",
        "state=clear:id=a",
    ]);
    assert_eq!(ids(&parser), vec![None, Some("ab".to_owned())]);
}

#[test]
fn clear_when_id_absent_from_store_then_seq_unchanged() {
    let mut parser = parser_with(&["state=working"]);
    let seq = parser.screen().program_status_seq();
    parser.process(&report("state=clear:id=nope"));
    assert_eq!(parser.screen().program_status_seq(), seq);
}

#[test]
fn sequence_when_at_4096_bytes_then_accepted() {
    // 7 bytes "ESC ] 7501 ;" + body + 2 bytes "ESC \".
    let pad = "x".repeat(4096 - 9 - "state=done:".len());
    assert_accepted(&format!("state=done:{pad}"));
}

#[test]
fn sequence_when_over_4096_bytes_then_rejected() {
    let pad = "x".repeat(4096 - 9 - "state=done:".len() + 1);
    assert_rejected(&format!("state=done:{pad}"));
}

#[test]
fn key_when_16_bytes_then_accepted() {
    assert_accepted(&format!("state=done:{}=1", "k".repeat(16)));
}

#[test]
fn key_when_17_bytes_then_rejected() {
    assert_rejected(&format!("state=done:{}=1", "k".repeat(17)));
}

#[test]
fn msg_when_2048_decoded_bytes_then_accepted() {
    let msg = b64(&[b'a'; 2048]);
    assert_eq!(msg.len(), 2732);
    let parser = parser_with(&[&format!("state=done:msg={msg}")]);
    assert_eq!(
        parser
            .screen()
            .program_status()
            .unwrap()
            .msg()
            .unwrap()
            .len(),
        2048
    );
}

#[test]
fn msg_when_2049_decoded_bytes_then_rejected() {
    let msg = b64(&[b'a'; 2049]);
    assert_rejected(&format!("state=done:msg={}", msg.trim_end_matches('=')));
}

#[test]
fn msg_when_over_2732_encoded_bytes_then_rejected() {
    assert_rejected(&format!("state=done:msg={}", "A".repeat(2733)));
}

#[test]
fn title_when_192_decoded_bytes_then_accepted() {
    let title = b64(&[b't'; 192]);
    assert_eq!(title.len(), 256);
    assert_accepted(&format!("state=done:title={title}"));
}

#[test]
fn title_when_193_decoded_bytes_then_rejected() {
    assert_rejected(&format!("state=done:title={}", b64(&[b't'; 193])));
}

#[test]
fn title_when_over_256_encoded_bytes_then_rejected() {
    assert_rejected(&format!("state=done:title={}", "A".repeat(257)));
}

#[test]
fn app_when_32_bytes_then_accepted() {
    let app = "a".repeat(32);
    let parser = parser_with(&[&format!("state=done:app={app}")]);
    assert_eq!(
        parser.screen().program_status().unwrap().app(),
        Some(app.as_str())
    );
}

#[test]
fn app_when_33_bytes_then_rejected() {
    assert_rejected(&format!("state=done:app={}", "a".repeat(33)));
}

#[test]
fn app_when_invalid_chars_then_absent() {
    let parser = parser_with(&["state=done:app=a/b"]);
    assert_eq!(parser.screen().program_status().unwrap().app(), None);
}

#[test]
fn id_when_at_every_limit_then_accepted() {
    let seg = "s".repeat(32);
    assert_accepted(&format!("state=done:id={seg}"));
    assert_accepted(&format!("state=done:id={}", ["a"; 8].join("/")));
    let total = format!("{seg}/{seg}/{seg}/{}", "s".repeat(29));
    assert_eq!(total.len(), 128);
    assert_accepted(&format!("state=done:id={total}"));
}

#[test]
fn id_when_segment_over_32_then_rejected() {
    assert_rejected(&format!("state=done:id={}", "s".repeat(33)));
}

#[test]
fn id_when_deeper_than_8_then_rejected() {
    assert_rejected(&format!("state=done:id={}", ["a"; 9].join("/")));
}

#[test]
fn id_when_over_128_total_then_rejected() {
    let seg = "s".repeat(32);
    assert_rejected(&format!(
        "state=done:id={seg}/{seg}/{seg}/{}",
        "s".repeat(30)
    ));
}

#[test]
fn id_when_malformed_then_report_ignored_not_applied_to_root() {
    for id in ["", "a//b", "/a", "a/", "a,b", "a=b"] {
        assert_rejected(&format!("state=done:id={id}"));
    }
}

#[test]
fn msg_when_bad_base64_then_rejected() {
    assert_rejected("state=done:msg=A");
    assert_rejected("state=done:msg=aG=k");
}

#[test]
fn msg_when_decodes_to_control_char_then_rejected() {
    for text in ["a\nb", "\0", "x\u{1b}[31m", "\u{7f}", "\u{85}", "\u{9f}"] {
        assert_rejected(&format!("state=done:msg={}", b64(text.as_bytes())));
    }
}

#[test]
fn title_when_decodes_to_control_char_then_rejected() {
    assert_rejected(&format!("state=done:title={}", b64(b"\ttab")));
}

#[test]
fn msg_when_valid_utf8_then_decoded() {
    let parser = parser_with(&[&format!("state=done:msg={}", b64("héllo ✓".as_bytes()))]);
    assert_eq!(
        parser.screen().program_status().unwrap().msg(),
        Some("héllo ✓")
    );
}

#[test]
fn kind_when_blocked_then_kept() {
    for (name, kind) in [
        ("permission", BlockedKind::Permission),
        ("question", BlockedKind::Question),
        ("auth", BlockedKind::Auth),
    ] {
        let parser = parser_with(&[&format!("state=blocked:kind={name}")]);
        assert_eq!(parser.screen().program_status().unwrap().kind(), Some(kind));
    }
}

#[test]
fn kind_when_not_blocked_or_unknown_then_absent() {
    let parser = parser_with(&["state=working:kind=auth"]);
    assert_eq!(parser.screen().program_status().unwrap().kind(), None);
    let parser = parser_with(&["state=blocked:kind=coffee"]);
    assert_eq!(parser.screen().program_status().unwrap().kind(), None);
}

#[test]
fn progress_when_in_range_on_working_or_blocked_then_kept() {
    for body in ["state=working:progress=0", "state=blocked:progress=100"] {
        let parser = parser_with(&[body]);
        assert!(parser
            .screen()
            .program_status()
            .unwrap()
            .progress()
            .is_some());
    }
}

#[test]
fn progress_when_out_of_range_or_wrong_state_then_absent() {
    for body in [
        "state=working:progress=101",
        "state=working:progress=-1",
        "state=working:progress=5.5",
        "state=done:progress=50",
        "state=idle:progress=50",
    ] {
        let parser = parser_with(&[body]);
        assert_eq!(
            parser.screen().program_status().unwrap().progress(),
            None,
            "{body}"
        );
    }
}

#[test]
fn app_when_record_has_none_then_inherits_nearest_ancestor() {
    let parser = parser_with(&[
        "state=working:app=deploy",
        "state=working:id=eu",
        "state=working:id=us:app=brew",
        "state=working:id=us/east/1",
    ]);
    let screen = parser.screen();
    assert_eq!(screen.program_status_app(Some("eu")), Some("deploy"));
    assert_eq!(screen.program_status_app(Some("us/east/1")), Some("brew"));
    assert_eq!(screen.program_status_app(None), Some("deploy"));
    assert_eq!(screen.program_status_app(Some("missing")), None);
}

#[test]
fn insert_when_store_full_then_least_recently_updated_evicted() {
    let mut parser = Parser::new(24, 80, 0);
    for i in 0..256 {
        parser.process(&report(&format!("state=working:id=r{i}")));
    }
    parser.process(&report("state=done:id=r0"));
    parser.process(&report("state=working:id=new"));
    let ids = ids(&parser);
    assert_eq!(ids.len(), 256);
    assert!(ids.contains(&Some("r0".to_owned())));
    assert!(ids.contains(&Some("new".to_owned())));
    assert!(!ids.contains(&Some("r1".to_owned())));
}

#[test]
fn update_when_store_full_then_nothing_evicted() {
    let mut parser = Parser::new(24, 80, 0);
    for i in 0..256 {
        parser.process(&report(&format!("state=working:id=r{i}")));
    }
    parser.process(&report("state=done:id=r5"));
    assert_eq!(ids(&parser).len(), 256);
}

fn mixed_states() -> Parser {
    parser_with(&[
        "state=working",
        "state=blocked:id=b",
        "state=idle:id=i",
        "state=done:id=d",
        "state=error:id=e",
    ])
}

#[test]
fn prompt_start_when_osc_133_a_then_only_done_and_error_survive() {
    let mut parser = mixed_states();
    parser.process(b"\x1b]133;A\x07");
    assert_eq!(
        ids(&parser),
        vec![Some("d".to_owned()), Some("e".to_owned())]
    );
}

#[test]
fn osc_133_when_not_prompt_start_then_records_untouched() {
    let mut parser = mixed_states();
    parser.process(b"\x1b]133;B\x07\x1b]133;D;0\x07");
    assert_eq!(ids(&parser).len(), 5);
}

#[test]
fn process_exit_when_called_then_only_done_and_error_survive() {
    let mut parser = mixed_states();
    parser.screen_mut().program_status_on_process_exit();
    assert_eq!(
        ids(&parser),
        vec![Some("d".to_owned()), Some("e".to_owned())]
    );
}

#[test]
fn prompt_start_when_nothing_to_drop_then_seq_unchanged() {
    let mut parser = parser_with(&["state=done"]);
    let seq = parser.screen().program_status_seq();
    parser.process(b"\x1b]133;A\x07");
    assert_eq!(parser.screen().program_status_seq(), seq);
}

#[test]
fn ris_when_records_exist_then_clears_them_and_seq_advances() {
    let mut parser = mixed_states();
    let seq = parser.screen().program_status_seq();
    parser.process(b"\x1bc");
    assert!(ids(&parser).is_empty());
    assert!(!parser.screen().program_status_seen());
    assert!(parser.screen().program_status_seq() > seq);
}

#[test]
fn decstr_when_received_then_records_survive() {
    let mut parser = mixed_states();
    parser.process(b"\x1b[!p");
    assert_eq!(ids(&parser).len(), 5);
}

#[test]
fn alternate_screen_when_switched_then_records_survive() {
    let mut parser = parser_with(&["state=working"]);
    parser.process(b"\x1b[?1049h");
    parser.process(&report("state=blocked:id=x"));
    parser.process(b"\x1b[?1049l");
    assert_eq!(ids(&parser), vec![None, Some("x".to_owned())]);
}

#[test]
fn seq_when_reports_applied_then_increments_once_each() {
    let mut parser = Parser::new(24, 80, 0);
    parser.process(&report("state=working"));
    assert_eq!(parser.screen().program_status_seq(), 1);
    parser.process(&report("state=bogus"));
    parser.process(&report("?"));
    assert_eq!(parser.screen().program_status_seq(), 1);
    parser.process(&report("state=done"));
    parser.process(&report("state=clear"));
    assert_eq!(parser.screen().program_status_seq(), 3);
}

#[test]
fn seen_when_valid_report_applied_then_true() {
    assert!(!Parser::new(24, 80, 0).screen().program_status_seen());
    assert!(parser_with(&["state=idle"]).screen().program_status_seen());
}

// Shapes emitted by Claude Code 2.1.295.

#[test]
fn claude_code_when_root_and_child_tasks_reported_then_all_recorded() {
    let title = b64(b"Fix the parser");
    let msg = b64(b"Waiting for your answer");
    let parser = parser_with(&[
        &format!("state=working:app=claude-code:progress=40:title={title}"),
        &format!("state=blocked:app=claude-code:id=task_1.a:kind=question:msg={msg}"),
        "state=blocked:app=claude-code:id=plan-2:kind=permission",
        "state=blocked:app=claude-code:id=paused+3",
        "state=working:app=claude-code:id=bg4",
    ]);
    let screen = parser.screen();
    let root = screen.program_status().unwrap();
    assert_eq!(
        (root.state(), root.progress()),
        (ProgramState::Working, Some(40))
    );
    assert_eq!(root.title(), Some("Fix the parser"));
    let kinds: Vec<_> = screen
        .program_status_records()
        .map(|(_, r)| r.kind())
        .collect();
    assert_eq!(
        kinds,
        vec![
            None,
            None,
            None,
            Some(BlockedKind::Permission),
            Some(BlockedKind::Question)
        ]
    );
    assert_eq!(screen.program_status_app(Some("bg4")), Some("claude-code"));
}

#[test]
fn claude_code_when_clear_one_then_only_that_child_removed() {
    let parser = parser_with(&[
        "state=working:app=claude-code",
        "state=working:app=claude-code:id=bg4",
        "state=clear:id=bg4",
    ]);
    assert_eq!(ids(&parser), vec![None]);
}

#[test]
fn claude_code_when_clear_all_on_exit_then_store_empty() {
    let parser = parser_with(&[
        "state=done:app=claude-code",
        "state=working:app=claude-code:id=bg4",
        "state=clear",
    ]);
    assert!(ids(&parser).is_empty());
}

#[test]
fn is_descendant_when_prefix_without_slash_then_false() {
    assert!(super::is_descendant("a/b", "a"));
    assert!(!super::is_descendant("ab", "a"));
    assert!(!super::is_descendant("a", "a"));
}
