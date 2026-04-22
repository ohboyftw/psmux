use psmux_fleet::detector::{AwaitingDetector, RegexDetector};
use psmux_fleet::AgentKind;
use std::time::Duration;

#[test]
fn claude_confirmation_prompt_detected() {
    let d = RegexDetector::new_with_defaults();
    let text = "Do something?\nDo you want to proceed?\n";
    let v = d.detect(AgentKind::Claude, text);
    assert!(v.is_some());
    let reason = v.unwrap();
    assert!(reason.contains("Do you want to proceed"));
}

#[test]
fn shell_prompt_detected() {
    let d = RegexDetector::new_with_defaults();
    assert!(d.detect(AgentKind::Shell, "$ ").is_some());
    assert!(d.detect(AgentKind::Shell, "PS C:\\Users> ").is_some());
    assert!(d.detect(AgentKind::Shell, "> ").is_some());
}

#[test]
fn autonomous_prose_is_not_a_match() {
    // OMC's d6354c8 regression guard.
    let d = RegexDetector::new_with_defaults();
    let text = "Running autonomous agents in parallel. No prompt visible.";
    assert!(d.detect(AgentKind::Claude, text).is_none());
}

#[test]
fn debounce_caps_to_2_hz_per_workspace() {
    let d = RegexDetector::new_with_defaults();
    let ws = "w1";
    assert!(d.try_detect(ws, AgentKind::Claude, "[y/n]"));
    // Second call within 500ms should be debounced (returns false without re-evaluating)
    assert!(!d.try_detect(ws, AgentKind::Claude, "[y/n]"));
    std::thread::sleep(Duration::from_millis(520));
    assert!(d.try_detect(ws, AgentKind::Claude, "[y/n]"));
}
