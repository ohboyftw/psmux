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
