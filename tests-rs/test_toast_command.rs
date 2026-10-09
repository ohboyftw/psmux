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
