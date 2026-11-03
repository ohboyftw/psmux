//! A quoted control-line argument is a value, never a flag. Included from
//! `src/server/spawn_args.rs`.

use super::*;

const NEW_WINDOW_VALUES: &[&str] = &["-n", "-c", "-F", "-e", "--shell"];

/// `new-window -n "<name>" -c "D:\x" "claude --continue"` as the restore step
/// sends it: every value quoted.
fn restore_line(name: &str) -> (Vec<&str>, Vec<bool>) {
    (
        vec!["-n", name, "-c", r"D:\x", "claude --continue"],
        vec![false, true, false, true, true],
    )
}

#[test]
fn spawn_args_when_name_is_a_flag_token_then_it_stays_the_name() {
    for name in ["-c", "-t", "-d", "-P", "--", "--shell", "-e", "-n"] {
        let (args, quoted) = restore_line(name);
        let parsed = parse_spawn_args(&args, &quoted, NEW_WINDOW_VALUES);
        assert_eq!(parsed.value("-n"), Some(name), "{name}");
        assert_eq!(parsed.value("-c"), Some(r"D:\x"), "{name}");
        assert_eq!(parsed.value("--shell"), None, "{name}");
        assert!(!parsed.has("-d"), "{name}");
        assert!(!parsed.has("-P"), "{name}");
        assert_eq!(
            parsed.command.as_deref(),
            Some("claude --continue"),
            "{name}"
        );
    }
}

#[test]
fn spawn_args_when_unquoted_flags_then_parsed_as_before() {
    let args = [
        "-d",
        "-P",
        "-F",
        "#{pane_id}",
        "-c",
        "C:/w",
        "-e",
        "A=1",
        "-e",
        "B=2",
        "pwsh",
    ];
    let quoted = [false; 11];
    let parsed = parse_spawn_args(&args, &quoted, NEW_WINDOW_VALUES);
    assert!(parsed.has("-d"));
    assert!(parsed.has("-P"));
    assert_eq!(parsed.value("-F"), Some("#{pane_id}"));
    assert_eq!(parsed.value("-c"), Some("C:/w"));
    assert_eq!(parsed.values("-e").collect::<Vec<_>>(), ["A=1", "B=2"]);
    assert_eq!(parsed.command.as_deref(), Some("pwsh"));
}

#[test]
fn spawn_args_when_unquoted_double_dash_then_rest_is_the_command() {
    let args = ["-c", "C:/w", "--", "ssh", "-c", "x"];
    let parsed = parse_spawn_args(&args, &[false; 6], NEW_WINDOW_VALUES);
    assert_eq!(parsed.value("-c"), Some("C:/w"));
    assert_eq!(parsed.command.as_deref(), Some("ssh -c x"));
}

#[test]
fn target_positions_when_quoted_dash_t_then_not_a_target() {
    let (args, quoted) = restore_line("-t");
    assert!(target_flag_positions(&args, &quoted).is_empty());
}

#[test]
fn target_positions_when_unquoted_dash_t_before_double_dash_then_found() {
    let args = ["-t", "%3", "--", "-t", "x"];
    assert_eq!(target_flag_positions(&args, &[false; 5]), [0]);
}
