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

fn snapshot_with_first_pane(pc: PaneCommand) -> SessionSnapshot {
    SessionSnapshot {
        version: 1,
        session_name: "s".into(),
        timestamp: 1,
        windows: vec![WindowSnapshot {
            name: "w".into(),
            id: 0,
            active_path: vec![],
            layout_tree: LayoutTreeNode::Leaf { id: 0 },
            pane_commands: vec![pc],
        }],
        active_window_idx: 0,
    }
}

#[test]
fn first_pane_args_when_agent_pane_then_command_is_positional_not_raw() {
    let snap = snapshot_with_first_pane(pane(Some("pwsh"), "claude-code", None));
    assert_eq!(
        first_pane_new_session_args("s", &snap),
        [
            "new-session",
            "-d",
            "-s",
            "s",
            "-c",
            "C:/work",
            "claude --continue"
        ]
    );
}

#[test]
fn first_pane_args_when_shell_pane_then_no_command() {
    let snap = snapshot_with_first_pane(pane(None, "", None));
    assert_eq!(
        first_pane_new_session_args("s", &snap),
        ["new-session", "-d", "-s", "s", "-c", "C:/work"]
    );
}

fn window(name: &str, panes: Vec<PaneCommand>) -> WindowSnapshot {
    WindowSnapshot {
        name: name.into(),
        id: 0,
        active_path: vec![],
        layout_tree: LayoutTreeNode::Leaf { id: 0 },
        pane_commands: panes,
    }
}

fn snapshot_of(windows: Vec<WindowSnapshot>) -> SessionSnapshot {
    SessionSnapshot {
        version: 1,
        session_name: "s".into(),
        timestamp: 1,
        windows,
        active_window_idx: 0,
    }
}

#[test]
fn restore_steps_when_one_pane_without_name_then_nothing_to_do() {
    let snap = snapshot_of(vec![window("", vec![pane(None, "", None)])]);
    assert!(restore_steps("s", &snap).is_empty());
}

#[test]
fn restore_steps_when_second_window_then_new_window_with_its_cwd_and_resume() {
    let snap = snapshot_of(vec![
        window("", vec![pane(None, "", None)]),
        window("", vec![pane(Some("pwsh"), "claude-code", None)]),
    ]);
    assert_eq!(
        restore_steps("s", &snap),
        [[
            "-t",
            "s",
            "new-window",
            "-c",
            "C:/work",
            "claude --continue"
        ]]
    );
}

#[test]
fn restore_steps_when_window_has_two_panes_then_split_and_tile() {
    let snap = snapshot_of(vec![window(
        "",
        vec![pane(None, "", None), pane(None, "pi", None)],
    )]);
    assert_eq!(
        restore_steps("s", &snap),
        [
            vec!["-t", "s", "split-window", "-c", "C:/work", "pi --continue"],
            vec!["-t", "s", "select-layout", "tiled"],
        ]
    );
}

#[test]
fn restore_steps_when_window_named_then_rename_quoted_for_the_server() {
    let snap = snapshot_of(vec![window(r#"my "w""#, vec![pane(None, "", None)])]);
    let steps = restore_steps("s", &snap);
    assert_eq!(steps, [["-t", "s", "rename-window", r#""my \"w\"""#]]);
    let wire = format!("rename-window {}", steps[0][3]);
    assert_eq!(
        crate::commands::parse_command_line(&wire),
        ["rename-window", r#"my "w""#]
    );
}

#[test]
fn restore_steps_when_any_snapshot_then_every_step_names_the_session_first() {
    let snap = snapshot_of(vec![
        window("a", vec![pane(None, "", None), pane(None, "", None)]),
        window("b", vec![pane(None, "", None), pane(None, "", None)]),
    ]);
    let steps = restore_steps("s", &snap);
    assert_eq!(steps.len(), 7);
    assert!(steps.iter().all(|s| s[..2] == ["-t", "s"]), "{steps:?}");
}

fn transition(
    old: Option<vt100::ProgramState>,
    new: Option<vt100::ProgramState>,
) -> crate::agent_state::AgentTransition {
    crate::agent_state::AgentTransition {
        old,
        new,
        record_id: None,
        details: Default::default(),
    }
}

#[test]
fn changes_resume_source_when_state_flips_then_false() {
    use vt100::ProgramState::{Idle, Working};
    assert!(!changes_resume_source(&transition(
        Some(Working),
        Some(Idle)
    )));
}

#[test]
fn changes_resume_source_when_agent_appears_then_true() {
    let t = transition(None, Some(vt100::ProgramState::Working));
    assert!(changes_resume_source(&t));
}

#[test]
fn changes_resume_source_when_agent_leaves_then_true() {
    let t = transition(Some(vt100::ProgramState::Blocked), None);
    assert!(changes_resume_source(&t));
}

fn row(pid: u32, ppid: u32, name: &str) -> ProcessRow {
    (pid, ppid, name.into())
}

#[test]
fn agent_pid_when_agent_has_deeper_tool_children_then_agent_itself() {
    let table = [
        row(10, 1, "pwsh.exe"),
        row(11, 10, "conhost.exe"),
        row(20, 10, "claude.exe"),
        row(30, 20, "bash.exe"),
        row(40, 30, "cargo.exe"),
        row(50, 20, "node.exe"),
    ];
    assert_eq!(agent_pid(10, &table), Some(20));
}

#[test]
fn agent_pid_when_npm_shim_runs_through_cmd_then_node_below_it() {
    let table = [
        row(10, 1, "pwsh.exe"),
        row(15, 10, "cmd.exe"),
        row(16, 15, "node.exe"),
    ];
    assert_eq!(agent_pid(10, &table), Some(16));
}

#[test]
fn agent_pid_when_only_console_host_below_shell_then_none() {
    let table = [row(10, 1, "pwsh.exe"), row(11, 10, "conhost.exe")];
    assert_eq!(agent_pid(10, &table), None);
}

#[test]
fn agent_pid_when_pane_spawned_agent_directly_then_root() {
    let table = [row(20, 1, "claude.exe"), row(30, 20, "node.exe")];
    assert_eq!(agent_pid(20, &table), Some(20));
}

#[test]
fn agent_pid_when_root_has_exited_then_none() {
    assert_eq!(agent_pid(10, &[row(20, 1, "claude.exe")]), None);
}
