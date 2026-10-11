use super::*;
use crate::agent_state::AgentRecordDetails;

fn transition(old: Option<ProgramState>, new: Option<ProgramState>) -> AgentTransition {
    AgentTransition {
        old,
        new,
        record_id: None,
        details: AgentRecordDetails::default(),
    }
}

fn blocked(kind: Option<BlockedKind>, msg: &str) -> AgentTransition {
    let mut t = transition(Some(ProgramState::Working), Some(ProgramState::Blocked));
    t.details.kind = kind;
    t.details.app = "claude-code".into();
    t.details.msg = msg.into();
    t
}

/// Fires every due toast at `now`, as the server tick does when no other
/// toast is in flight, and returns how many fired.
fn fire_all(toasts: &mut AgentToasts, now: Instant) -> usize {
    let mut fired = 0;
    while let Some(id) = toasts.due(now).map(|t| t.pane_id) {
        toasts.mark_fired(id, now);
        fired += 1;
    }
    fired
}

#[test]
fn offer_when_working_idle_flips_then_nothing_pending() {
    let mut toasts = AgentToasts::default();
    let now = Instant::now();
    for i in 0..10 {
        let (a, b) = if i % 2 == 0 {
            (ProgramState::Working, ProgramState::Idle)
        } else {
            (ProgramState::Idle, ProgramState::Working)
        };
        toasts.offer(1, &transition(Some(a), Some(b)), "s", now);
    }
    assert_eq!(toasts.pending_len(), 0);
    assert_eq!(fire_all(&mut toasts, now), 0);
}

#[test]
fn offer_when_blocked_permission_then_one_toast_names_kind() {
    let mut toasts = AgentToasts::default();
    let now = Instant::now();
    toasts.offer(
        3,
        &blocked(Some(BlockedKind::Permission), "Run rm?"),
        "work",
        now,
    );
    let toast = toasts.due(now).expect("due").clone();
    assert_eq!(toast.title, "claude-code needs permission");
    assert_eq!(toast.body, "work %3: Run rm?");
    assert_eq!(fire_all(&mut toasts, now), 1);
}

#[test]
fn due_when_fire_failed_then_blocked_still_delivered_next_tick() {
    let mut toasts = AgentToasts::default();
    let now = Instant::now();
    toasts.offer(1, &blocked(None, ""), "s", now);
    // The tick saw another toast in flight and did not call mark_fired.
    assert!(toasts.due(now).is_some());
    let later = now + Duration::from_millis(100);
    assert_eq!(
        toasts.due(later).map(|t| t.state),
        Some(ProgramState::Blocked)
    );
    assert_eq!(fire_all(&mut toasts, later), 1);
}

#[test]
fn offer_when_disabled_then_no_toast() {
    let mut toasts = AgentToasts::default();
    toasts.set_enabled(false);
    let now = Instant::now();
    toasts.offer(1, &blocked(None, ""), "s", now);
    assert_eq!(fire_all(&mut toasts, now), 0);
}

#[test]
fn set_enabled_when_turned_off_then_pending_dropped() {
    let mut toasts = AgentToasts::default();
    let now = Instant::now();
    toasts.offer(1, &blocked(None, ""), "s", now);
    toasts.set_enabled(false);
    toasts.set_enabled(true);
    assert_eq!(toasts.pending_len(), 0);
}

#[test]
fn offer_when_same_state_reentered_within_window_then_deduped() {
    let mut toasts = AgentToasts::default();
    let now = Instant::now();
    let done = transition(Some(ProgramState::Working), Some(ProgramState::Done));
    toasts.offer(1, &done, "s", now);
    assert_eq!(fire_all(&mut toasts, now), 1);
    let again = now + Duration::from_secs(10);
    toasts.offer(1, &done, "s", again);
    assert_eq!(toasts.pending_len(), 0);
}

#[test]
fn offer_when_same_state_after_window_then_toasted_again() {
    let mut toasts = AgentToasts::default();
    let now = Instant::now();
    let done = transition(Some(ProgramState::Working), Some(ProgramState::Done));
    toasts.offer(1, &done, "s", now);
    fire_all(&mut toasts, now);
    let later = now + DEDUPE_WINDOW;
    toasts.offer(1, &done, "s", later);
    assert_eq!(fire_all(&mut toasts, later), 1);
}

#[test]
fn due_when_pane_toasted_recently_then_next_state_waits_not_dropped() {
    let mut toasts = AgentToasts::default();
    let now = Instant::now();
    let done = transition(Some(ProgramState::Working), Some(ProgramState::Done));
    toasts.offer(1, &done, "s", now);
    fire_all(&mut toasts, now);
    let soon = now + Duration::from_secs(2);
    let error = transition(Some(ProgramState::Done), Some(ProgramState::Error));
    toasts.offer(1, &error, "s", soon);
    assert!(toasts.due(soon).is_none());
    let state = toasts.due(now + PANE_MIN_INTERVAL).map(|t| t.state);
    assert_eq!(state, Some(ProgramState::Error));
}

#[test]
fn due_when_two_panes_pending_then_blocked_first_and_both_kept() {
    let mut toasts = AgentToasts::default();
    let now = Instant::now();
    let done = transition(Some(ProgramState::Working), Some(ProgramState::Done));
    toasts.offer(1, &done, "s", now);
    toasts.offer(2, &blocked(None, ""), "s", now);
    assert_eq!(toasts.due(now).map(|t| t.pane_id), Some(2));
    toasts.mark_fired(2, now);
    assert!(toasts.due(now).is_none(), "global minimum interval");
    let later = now + MIN_INTERVAL;
    assert_eq!(toasts.due(later).map(|t| t.pane_id), Some(1));
}

#[test]
fn offer_when_pane_leaves_blocked_before_fire_then_toast_dropped() {
    let mut toasts = AgentToasts::default();
    let now = Instant::now();
    toasts.offer(1, &blocked(None, ""), "s", now);
    let resumed = transition(Some(ProgramState::Blocked), Some(ProgramState::Working));
    toasts.offer(1, &resumed, "s", now);
    assert_eq!(toasts.pending_len(), 0);
}

#[test]
fn offer_when_done_pending_then_exit_keeps_it() {
    let mut toasts = AgentToasts::default();
    let now = Instant::now();
    let done = transition(Some(ProgramState::Working), Some(ProgramState::Done));
    toasts.offer(1, &done, "s", now);
    let gone = transition(Some(ProgramState::Done), None);
    toasts.offer(1, &gone, "s", now);
    assert_eq!(fire_all(&mut toasts, now), 1);
}

#[test]
fn build_toast_when_no_app_or_text_then_generic_title_and_place_only() {
    let mut toasts = AgentToasts::default();
    let now = Instant::now();
    let err = transition(Some(ProgramState::Working), Some(ProgramState::Error));
    toasts.offer(7, &err, "main", now);
    let toast = toasts.due(now).expect("due");
    assert_eq!(toast.title, "Agent reported an error");
    assert_eq!(toast.body, "main %7");
}
