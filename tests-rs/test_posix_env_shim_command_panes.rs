use super::*;

/// POSIX `env` shim coverage for panes spawned WITH a command.
///
/// psmux presents itself as tmux (`tmux -V` → "tmux 3.2"), so callers driving
/// it through the tmux surface emit POSIX command strings.  Claude Code
/// launches an agent-teams teammate as:
///
///   respawn-pane -k -t %N -- "cd <dir> && env VAR=VAL claude --agent-id ..."
///
/// `build_psrl_init` installs ENV_SHIM_PS for interactive panes, but the
/// `command.is_some()` branch of `build_command` used to bypass it entirely,
/// so `env` was undefined and every teammate pane died on spawn with
/// "env: The term 'env' is not recognized".  `respawn-pane` still returned 0,
/// so the failure was silent.

fn argv_of(b: &portable_pty::CommandBuilder) -> Vec<String> {
    b.get_argv()
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn prepend_env_shim_defines_env_function() {
    let out = prepend_env_shim("cd D:/x && env FOO=1 claude --agent-id a", true);
    assert!(
        out.contains("function Global:env"),
        "shim must define the env function, got: {out}"
    );
    assert!(
        out.ends_with("cd D:/x && env FOO=1 claude --agent-id a"),
        "the caller's command must be preserved verbatim at the end, got: {out}"
    );
}

#[test]
fn prepend_env_shim_is_a_noop_when_disabled() {
    let cmd = "cd D:/x && env FOO=1 claude";
    assert_eq!(prepend_env_shim(cmd, false), cmd);
}

#[test]
fn prepend_env_shim_does_not_double_separator() {
    // ENV_SHIM_PS is self-terminating ("...; ") — prepending must not
    // introduce an empty statement, which pwsh rejects with a parse error.
    let out = prepend_env_shim("echo hi", true);
    assert!(!out.contains(";;"), "double separator in: {out}");
    assert!(!out.contains("; ; "), "empty statement in: {out}");
}

#[test]
#[cfg(windows)]
fn command_pane_installs_env_shim_in_spawn_args() {
    // The regression: a pane spawned WITH a command must carry the shim.
    let b = build_command(
        Some("cd D:/x && env CLAUDE_CODE_AGENT_ID=1 claude --agent-id a"),
        None,
        true,
        false,
        "sess",
    );
    let argv = argv_of(&b);
    assert!(
        argv.iter().any(|a| a.contains("function Global:env")),
        "command panes must install the env shim, got argv: {argv:?}"
    );
    assert!(
        argv.iter().any(|a| a.contains("claude --agent-id a")),
        "the caller's command must survive, got argv: {argv:?}"
    );
}

#[test]
#[cfg(windows)]
fn command_pane_omits_env_shim_when_env_shim_off() {
    let b = build_command(Some("echo hi"), None, false, false, "sess");
    let argv = argv_of(&b);
    assert!(
        !argv.iter().any(|a| a.contains("function Global:env")),
        "env-shim=off must not inject the shim, got argv: {argv:?}"
    );
}

/// Fix B: `--shell` / `default-shell` were resolved and then discarded for
/// panes spawned with a command, which always fell back to `cached_shell()`
/// (hardcoded pwsh → cmd).  A POSIX shell must reach the `-c` branch.
#[test]
#[cfg(windows)]
fn command_pane_honours_shell_override_with_posix_shell() {
    let bash = "C:/Program Files/Git/bin/bash.exe";
    if !std::path::Path::new(bash).is_file() {
        eprintln!("skipping: Git Bash not installed at {bash}");
        return;
    }
    let b = build_command(Some("env FOO=1 claude"), Some(bash), true, false, "sess");
    let argv = argv_of(&b);
    assert!(
        b.get_shell().to_lowercase().contains("bash") || argv.iter().any(|a| a.contains("-c")),
        "shell override must route to the POSIX -c branch, got shell={} argv={argv:?}",
        b.get_shell()
    );
    // A real POSIX shell has a real `env`; the PowerShell shim must not leak in.
    assert!(
        !argv.iter().any(|a| a.contains("function Global:env")),
        "the PS env shim must not be injected into a POSIX shell, got argv: {argv:?}"
    );
}

#[test]
#[cfg(windows)]
fn command_pane_falls_back_to_system_shell_when_override_empty() {
    // `default-shell` unset expands to "" — must not be treated as a shell.
    let b = build_command(Some("echo hi"), Some(""), true, false, "sess");
    let shell = b.get_shell().to_lowercase();
    assert!(
        shell.contains("pwsh") || shell.contains("cmd") || shell.contains("powershell"),
        "empty override must fall back to the system shell, got: {shell}"
    );
}
