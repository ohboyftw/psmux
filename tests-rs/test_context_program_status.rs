//! `ContextInfo.program_status` in the `list` response (ticket 04b).

use super::*;
use crate::agent_state::AgentStateCache;

fn context(cache: &AgentStateCache) -> serde_json::Value {
    let info = ContextInfo {
        context_id: "%3".into(),
        alive: true,
        cwd: None,
        title: None,
        shell_name: None,
        metadata: None,
        program_status: ProgramStatusInfo::from_cache(cache),
    };
    serde_json::to_value(&info).expect("ContextInfo must serialize")
}

fn report(parser: &mut vt100::Parser, body: &str) {
    parser.process(format!("\x1b]7501;{body}\x1b\\").as_bytes());
}

#[test]
fn list_when_pane_never_reported_then_program_status_absent() {
    let json = context(&AgentStateCache::default());
    assert!(
        !json
            .as_object()
            .expect("object")
            .contains_key("program_status"),
        "a non-reporting pane must not carry \"program_status\":null: {json}"
    );
}

#[test]
fn list_when_child_blocked_under_working_root_then_reports_child() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut parser, "state=working:app=claude-code:progress=40");
    report(&mut parser, "id=t1:state=blocked:kind=permission");
    cache.observe(parser.screen());
    let ps = &context(&cache)["program_status"];
    assert_eq!(ps["state"], "blocked");
    assert_eq!(ps["record_id"], "t1");
    assert_eq!(ps["kind"], "permission");
    assert_eq!(ps["app"], "claude-code");
}

#[test]
fn list_when_root_working_with_progress_then_only_set_keys_present() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut parser, "state=working:progress=40:msg=aGk=");
    cache.observe(parser.screen());
    let ps = context(&cache)["program_status"].clone();
    assert_eq!(ps["state"], "working");
    assert_eq!(ps["progress"], 40);
    for key in ["record_id", "kind", "app", "msg", "title"] {
        assert!(ps.get(key).is_none(), "{key} must be absent: {ps}");
    }
}

#[test]
fn list_when_state_cleared_then_program_status_absent() {
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut parser, "state=working");
    cache.observe(parser.screen());
    report(&mut parser, "state=clear");
    cache.observe(parser.screen());
    assert!(context(&cache).get("program_status").is_none());
}

#[test]
fn list_when_client_lacks_the_field_then_context_still_parses() {
    #[derive(serde::Deserialize)]
    struct OldContextInfo {
        context_id: String,
        alive: bool,
    }
    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut cache = AgentStateCache::default();
    report(&mut parser, "state=idle");
    cache.observe(parser.screen());
    let text = context(&cache).to_string();
    assert!(text.contains("\"program_status\""));
    let old: OldContextInfo = serde_json::from_str(&text).expect("old client must parse");
    assert_eq!(old.context_id, "%3");
    assert!(old.alive);
}
