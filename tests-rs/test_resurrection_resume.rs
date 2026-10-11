use super::*;

fn pane(command: Option<&str>, app: &str, resume: Option<&str>) -> PaneCommand {
    PaneCommand {
        command: command.map(str::to_string),
        cwd: "C:/work".into(),
        env: vec![],
        app: app.into(),
        resume_command: resume.map(str::to_string),
    }
}

#[test]
fn load_when_snapshot_predates_app_fields_then_defaults_to_empty() {
    let json = r#"{"version":1,"session_name":"old","timestamp":1,"active_window_idx":0,
        "windows":[{"name":"w","id":0,"active_path":[],"layout_tree":{"type":"leaf","id":0},
        "pane_commands":[{"command":"nvim","cwd":"C:/x","env":[]}]}]}"#;
    let snap: SessionSnapshot = serde_json::from_str(json).expect("old snapshot loads");
    let pc = &snap.windows[0].pane_commands[0];
    assert_eq!(pc.app, "");
    assert_eq!(pc.resume_command, None);
    assert_eq!(restore_command(pc).as_deref(), Some("nvim"));
}

#[test]
fn serialize_when_no_agent_then_new_keys_are_absent() {
    let json = serde_json::to_string(&pane(None, "", None)).unwrap();
    assert!(!json.contains("\"app\""), "{json}");
    assert!(!json.contains("resume_command"), "{json}");
}

#[test]
fn resume_command_for_when_known_apps_then_continue_in_cwd() {
    assert_eq!(resume_command_for("claude-code"), Some("claude --continue"));
    assert_eq!(resume_command_for("pi"), Some("pi --continue"));
}

#[test]
fn resume_command_for_when_unknown_or_padded_app_then_none() {
    for app in [
        "",
        "claude",
        "claude-code ",
        "Claude-Code",
        "pi;calc",
        "evil",
    ] {
        assert_eq!(resume_command_for(app), None, "{app:?}");
    }
}

#[test]
fn restore_command_when_agent_app_then_table_beats_original_command() {
    let pc = pane(Some("pwsh"), "claude-code", Some("claude --continue"));
    assert_eq!(restore_command(&pc).as_deref(), Some("claude --continue"));
}

#[test]
fn restore_command_when_stored_resume_tampered_then_table_still_wins() {
    let pc = pane(None, "pi", Some("calc.exe"));
    assert_eq!(restore_command(&pc).as_deref(), Some("pi --continue"));
}

#[test]
fn restore_command_when_resume_without_known_app_then_ignored() {
    let pc = pane(Some("nvim"), "", Some("calc.exe"));
    assert_eq!(restore_command(&pc).as_deref(), Some("nvim"));
}

#[test]
fn restore_command_when_default_shell_pane_then_none() {
    assert_eq!(restore_command(&pane(None, "", None)), None);
}
