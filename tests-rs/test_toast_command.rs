use super::*;

const HOSTILE: &str = "x\u{2019}); Start-Process calc; (\u{2018}";

fn env_value(cmd: &std::process::Command, key: &str) -> Option<String> {
    cmd.get_envs()
        .find(|(k, _)| *k == key)
        .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
}

#[test]
fn toast_command_when_body_has_smart_quotes_then_script_text_is_fixed() {
    let cmd = toast_command("title", HOSTILE);
    let args: Vec<_> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args.last().map(String::as_str), Some(TOAST_SCRIPT));
    assert!(args.iter().all(|a| !a.contains("calc")));
}

#[test]
fn toast_command_when_built_then_body_travels_in_env() {
    let cmd = toast_command("title", HOSTILE);
    assert_eq!(
        env_value(&cmd, "PSMUX_TOAST_BODY").as_deref(),
        Some(HOSTILE)
    );
}

#[test]
fn toast_command_when_built_then_title_travels_in_env() {
    let cmd = toast_command(HOSTILE, "body");
    assert_eq!(
        env_value(&cmd, "PSMUX_TOAST_TITLE").as_deref(),
        Some(HOSTILE)
    );
}

fn hostile_agent_toast() -> crate::agent_toast::Toast {
    let mut t = crate::agent_state::AgentTransition {
        old: Some(vt100::ProgramState::Working),
        new: Some(vt100::ProgramState::Blocked),
        record_id: None,
        details: Default::default(),
    };
    t.details.app = format!("{HOSTILE}'; calc; '");
    t.details.msg = format!("'; calc; '{HOSTILE}");
    let mut toasts = crate::agent_toast::AgentToasts::default();
    let now = std::time::Instant::now();
    toasts.offer(1, &t, "s", now);
    toasts.due(now).expect("due").clone()
}

#[test]
fn toast_command_when_agent_text_is_hostile_then_script_text_is_fixed() {
    let toast = hostile_agent_toast();
    let cmd = toast_command(&toast.title, &toast.body);
    let args: Vec<_> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args.last().map(String::as_str), Some(TOAST_SCRIPT));
    assert!(args.iter().all(|a| !a.contains("calc")));
}

#[test]
fn toast_command_when_agent_text_is_hostile_then_text_travels_verbatim_in_env() {
    let toast = hostile_agent_toast();
    let cmd = toast_command(&toast.title, &toast.body);
    let body = env_value(&cmd, "PSMUX_TOAST_BODY").unwrap_or_default();
    assert!(body.ends_with(&format!("'; calc; '{HOSTILE}")));
    let title = env_value(&cmd, "PSMUX_TOAST_TITLE").unwrap_or_default();
    assert!(title.starts_with(&format!("{HOSTILE}'; calc; '")));
}
