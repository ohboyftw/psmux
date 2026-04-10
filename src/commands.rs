use std::io;
use std::time::Instant;

use crate::copy_mode::{
    capture_active_pane, enter_copy_mode, paste_latest, save_latest_buffer, switch_with_copy_save,
};
use crate::pane::{create_window, kill_active_pane, split_active};
use crate::session::{list_all_sessions_tree, send_control_to_port};
use crate::tree::{compute_rects, get_active_pane_id, kill_all_children};
use crate::types::{Action, AppState, FocusDir, LayoutKind, Menu, MenuItem, Mode, Node};
use crate::window_ops::toggle_zoom;

/// Parse a popup dimension spec: "80" (absolute) or "95%" (percentage of term_dim).
pub(crate) fn parse_popup_dim_local(spec: &str, term_dim: u16, default: u16) -> u16 {
    if let Some(pct_str) = spec.strip_suffix('%') {
        if let Ok(pct) = pct_str.parse::<u16>() {
            let pct = pct.min(100);
            (term_dim as u32 * pct as u32 / 100) as u16
        } else {
            default
        }
    } else {
        spec.parse().unwrap_or(default)
    }
}

/// Resolve the default shell binary and its arguments for run-shell invocations.
///
/// Returns `(program, args)` where `args` is typically `["-NoProfile", "-Command"]`
/// on Windows (pwsh/powershell) or `["-c"]` on Unix (sh).
pub fn resolve_run_shell() -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        use std::path::PathBuf;
        if let Ok(path) = which::which("pwsh") {
            return (
                path.to_string_lossy().into_owned(),
                vec!["-NoProfile".to_string(), "-Command".to_string()],
            );
        }
        if let Ok(path) = which::which("powershell") {
            return (
                path.to_string_lossy().into_owned(),
                vec!["-NoProfile".to_string(), "-Command".to_string()],
            );
        }
        if let Ok(system_root) =
            std::env::var("SystemRoot").or_else(|_| std::env::var("SYSTEMROOT"))
        {
            let powershell = PathBuf::from(&system_root)
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe");
            if powershell.is_file() {
                return (
                    powershell.to_string_lossy().into_owned(),
                    vec!["-NoProfile".to_string(), "-Command".to_string()],
                );
            }
            let cmd = PathBuf::from(&system_root).join("System32").join("cmd.exe");
            if cmd.is_file() {
                return (cmd.to_string_lossy().into_owned(), vec!["/c".to_string()]);
            }
        }
        if let Ok(comspec) = std::env::var("ComSpec").or_else(|_| std::env::var("COMSPEC")) {
            let trimmed = comspec.trim();
            if !trimmed.is_empty() {
                return (trimmed.to_string(), vec!["/c".to_string()]);
            }
        }
        ("cmd".to_string(), vec!["/c".to_string()])
    }
    #[cfg(not(windows))]
    {
        ("sh".to_string(), vec!["-c".to_string()])
    }
}

/// Build a `std::process::Command` for a run-shell invocation.
///
/// Avoids double-wrapping when the command already starts with a shell binary
/// (e.g., `pwsh -NoProfile -File script.ps1`). Also detects bare `.ps1` file
/// paths and uses `-File` instead of `-Command` for reliable path handling.
pub fn build_run_shell_command(shell_cmd: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        let lower = shell_cmd.trim_start().to_lowercase();

        // Case 1: Command already starts with a shell binary (pwsh, powershell, cmd).
        // Run it directly to avoid nesting `pwsh -Command "pwsh -File ..."`.
        if lower.starts_with("pwsh ")
            || lower.starts_with("pwsh.exe ")
            || lower.starts_with("powershell ")
            || lower.starts_with("powershell.exe ")
            || lower.starts_with("cmd ")
            || lower.starts_with("cmd.exe ")
        {
            let parts = parse_command_line(shell_cmd);
            if parts.len() >= 2 {
                let mut c = std::process::Command::new(&parts[0]);
                for p in &parts[1..] {
                    c.arg(p);
                }
                return c;
            }
        }

        // Case 2: Bare .ps1 file path (possibly with arguments after the path).
        // Use `-File` which handles paths with spaces correctly.
        let trimmed = shell_cmd.trim();
        let first_token = trimmed.split_whitespace().next().unwrap_or("");
        let first_unquoted = first_token.trim_matches('"').trim_matches('\'');
        if first_unquoted.ends_with(".ps1") && std::path::Path::new(first_unquoted).exists() {
            let shell = if which::which("pwsh").is_ok() {
                "pwsh"
            } else {
                "powershell"
            };
            let mut c = std::process::Command::new(shell);
            c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
            // Split remaining args so the .ps1 path and its arguments are separate args
            let parts = parse_command_line(trimmed);
            for p in &parts {
                c.arg(p);
            }
            return c;
        }

        // Case 3: Regular command string. Wrap in shell.
        let (shell_prog, shell_args) = resolve_run_shell();
        let mut c = std::process::Command::new(&shell_prog);
        for a in &shell_args {
            c.arg(a);
        }
        c.arg(shell_cmd);
        c
    }
    #[cfg(not(windows))]
    {
        let (shell_prog, shell_args) = resolve_run_shell();
        let mut c = std::process::Command::new(&shell_prog);
        for a in &shell_args {
            c.arg(a);
        }
        c.arg(shell_cmd);
        c
    }
}

/// Show text output in a popup overlay (used by list-* commands inside a session).
fn show_output_popup(app: &mut AppState, title: &str, output: String) {
    let lines: Vec<&str> = output.lines().collect();
    let width = lines.iter().map(|l| l.len()).max().unwrap_or(40).max(20) as u16 + 4;
    let height = (lines.len() as u16 + 2).max(5);
    app.mode = Mode::PopupMode {
        command: title.to_string(),
        output,
        process: None,
        width: width.min(120),
        height,
        close_on_exit: false,
        popup_pane: None,
        scroll_offset: 0,
    };
}

/// Generate list-windows output from AppState (tmux-compatible format).
fn generate_list_windows(app: &AppState) -> String {
    crate::util::list_windows_tmux(app)
}

/// Generate list-panes output from AppState.
fn generate_list_panes(app: &AppState) -> String {
    let win = &app.windows[app.active_idx];
    fn collect(node: &Node, panes: &mut Vec<(usize, u16, u16)>) {
        match node {
            Node::Leaf(p) => {
                panes.push((p.id, p.last_cols, p.last_rows));
            }
            Node::Split { children, .. } => {
                for c in children {
                    collect(c, panes);
                }
            }
        }
    }
    let mut panes = Vec::new();
    collect(&win.root, &mut panes);
    let active_id = get_active_pane_id(&win.root, &win.active_path);
    let mut output = String::new();
    for (pos, (id, cols, rows)) in panes.iter().enumerate() {
        let idx = pos + app.pane_base_index;
        let marker = if active_id == Some(*id) {
            " (active)"
        } else {
            ""
        };
        output.push_str(&format!(
            "{}: [{}x{}] [history {}/{}, 0 bytes] %{}{}\n",
            idx, cols, rows, app.history_limit, app.history_limit, id, marker
        ));
    }
    output
}

/// Generate list-clients output from AppState.
fn generate_list_clients(app: &AppState) -> String {
    format!(
        "/dev/pts/0: {}: {} [{}x{}] (utf8)\n",
        app.session_name,
        app.windows[app.active_idx].name,
        app.last_window_area.width,
        app.last_window_area.height
    )
}

/// Generate show-hooks output from AppState.
fn generate_show_hooks(app: &AppState) -> String {
    let mut output = String::new();
    for (name, commands) in &app.hooks {
        if commands.len() == 1 {
            output.push_str(&format!("{} -> {}\n", name, commands[0]));
        } else {
            for (i, cmd) in commands.iter().enumerate() {
                output.push_str(&format!("{}[{}] -> {}\n", name, i, cmd));
            }
        }
    }
    if output.is_empty() {
        output.push_str("(no hooks)\n");
    }
    output
}

/// Generate list-commands output.
fn generate_list_commands() -> String {
    crate::help::cli_command_lines().join("\n")
}

/// Build the choose-tree data for the WindowChooser mode.
pub fn build_choose_tree(app: &AppState) -> Vec<crate::session::TreeEntry> {
    let current_windows: Vec<(String, usize, String, bool)> = app
        .windows
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let panes = crate::tree::count_panes(&w.root);
            let size = format!(
                "{}x{}",
                app.last_window_area.width, app.last_window_area.height
            );
            (w.name.clone(), panes, size, i == app.active_idx)
        })
        .collect();
    list_all_sessions_tree(&app.session_name, &current_windows)
}

/// Extract a window index from a tmux-style target string.
/// Handles formats like "0", ":0", ":=0", "=0", stripping leading ':'/'=' chars.
fn parse_window_target(target: &str) -> Option<usize> {
    let s = target.trim_start_matches(':').trim_start_matches('=');
    s.parse::<usize>().ok()
}

/// Parse a command string to an Action
pub fn parse_command_to_action(cmd: &str) -> Option<Action> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }

    match parts[0] {
        "display-panes" | "displayp" => Some(Action::DisplayPanes),
        "new-window" | "neww" => {
            // If extra flags like -c, -d, -n, -F, -e or a shell command are present,
            // store as Command to preserve the full argument string (esp. -c for start dir).
            let has_extra = parts.len() > 1;
            if has_extra {
                Some(Action::Command(cmd.to_string()))
            } else {
                Some(Action::NewWindow)
            }
        }
        "split-window" | "splitw" => {
            // If extra flags like -c, -d, -p, -F, or a shell command are present,
            // store as Command to preserve the full argument string.
            let has_extra = parts.iter().any(|p| {
                matches!(
                    *p,
                    "-c" | "-d" | "-p" | "-l" | "-F" | "-P" | "-b" | "-f" | "-I" | "-Z" | "-e"
                )
            }) || parts
                .iter()
                .any(|p| !p.starts_with('-') && *p != "split-window" && *p != "splitw");
            if has_extra {
                Some(Action::Command(cmd.to_string()))
            } else if parts.contains(&"-h") {
                Some(Action::SplitHorizontal)
            } else {
                Some(Action::SplitVertical)
            }
        }
        "kill-pane" | "killp" => Some(Action::KillPane),
        "next-window" | "next" => Some(Action::NextWindow),
        "previous-window" | "prev" => Some(Action::PrevWindow),
        "copy-mode" => Some(Action::CopyMode),
        "hints" => Some(Action::Command("hints".to_string())),
        "paste-buffer" | "pasteb" => Some(Action::Paste),
        "detach-client" | "detach" => Some(Action::Detach),
        "rename-window" | "renamew" => Some(Action::RenameWindow),
        "choose-window" | "choose-tree" | "choose-session" => Some(Action::WindowChooser),
        "resize-pane" | "resizep" if parts.contains(&"-Z") => Some(Action::ZoomPane),
        "zoom-pane" => Some(Action::ZoomPane),
        "select-pane" | "selectp" => {
            if parts.contains(&"-U") {
                Some(Action::MoveFocus(FocusDir::Up))
            } else if parts.contains(&"-D") {
                Some(Action::MoveFocus(FocusDir::Down))
            } else if parts.contains(&"-L") {
                Some(Action::MoveFocus(FocusDir::Left))
            } else if parts.contains(&"-R") {
                Some(Action::MoveFocus(FocusDir::Right))
            } else {
                Some(Action::Command(cmd.to_string()))
            }
        }
        "last-window" | "last" => Some(Action::Command("last-window".to_string())),
        "last-pane" | "lastp" => Some(Action::Command("last-pane".to_string())),
        "swap-pane" | "swapp" => Some(Action::Command(cmd.to_string())),
        "resize-pane" | "resizep" => Some(Action::Command(cmd.to_string())),
        "rotate-window" | "rotatew" => Some(Action::Command(cmd.to_string())),
        "break-pane" | "breakp" => Some(Action::Command(cmd.to_string())),
        "respawn-pane" | "respawnp" => Some(Action::Command(cmd.to_string())),
        "respawn-window" | "respawnw" => Some(Action::Command(cmd.to_string())),
        "kill-window" | "killw" => Some(Action::Command(cmd.to_string())),
        "kill-session" | "kill-ses" => Some(Action::Command(cmd.to_string())),
        "kill-server" => Some(Action::Command(cmd.to_string())),
        "select-window" | "selectw" => Some(Action::Command(cmd.to_string())),
        "toggle-sync" => Some(Action::Command("toggle-sync".to_string())),
        "send-keys" | "send" => Some(Action::Command(cmd.to_string())),
        "send-prefix" => Some(Action::Command(cmd.to_string())),
        "set-option" | "set" | "setw" | "set-window-option" => {
            Some(Action::Command(cmd.to_string()))
        }
        "show-options" | "show" | "show-window-options" | "showw" => {
            Some(Action::Command(cmd.to_string()))
        }
        "source-file" | "source" => Some(Action::Command(cmd.to_string())),
        "select-layout" | "selectl" => Some(Action::Command(cmd.to_string())),
        "next-layout" | "nextl" => Some(Action::Command("next-layout".to_string())),
        "previous-layout" | "prevl" => Some(Action::Command("previous-layout".to_string())),
        "confirm-before" | "confirm" => Some(Action::Command(cmd.to_string())),
        "display-menu" | "menu" => Some(Action::Command(cmd.to_string())),
        "display-popup" | "popup" => Some(Action::Command(cmd.to_string())),
        "display-message" | "display" => Some(Action::Command(cmd.to_string())),
        "pipe-pane" | "pipep" => Some(Action::Command(cmd.to_string())),
        "rename-session" | "rename" => Some(Action::Command(cmd.to_string())),
        "clear-history" | "clearhist" => Some(Action::Command("clear-history".to_string())),
        "set-buffer" | "setb" => Some(Action::Command(cmd.to_string())),
        "delete-buffer" | "deleteb" => Some(Action::Command("delete-buffer".to_string())),
        "list-buffers" | "lsb" => Some(Action::Command(cmd.to_string())),
        "show-buffer" | "showb" => Some(Action::Command(cmd.to_string())),
        "choose-buffer" | "chooseb" => Some(Action::Command(cmd.to_string())),
        "load-buffer" | "loadb" => Some(Action::Command(cmd.to_string())),
        "save-buffer" | "saveb" => Some(Action::Command(cmd.to_string())),
        "capture-pane" | "capturep" => Some(Action::Command(cmd.to_string())),
        "list-windows" | "lsw" => Some(Action::Command(cmd.to_string())),
        "list-panes" | "lsp" => Some(Action::Command(cmd.to_string())),
        "list-clients" | "lsc" => Some(Action::Command(cmd.to_string())),
        "list-commands" | "lscm" => Some(Action::Command(cmd.to_string())),
        "list-keys" | "lsk" => Some(Action::Command(cmd.to_string())),
        "list-sessions" | "ls" => Some(Action::Command(cmd.to_string())),
        "show-hooks" => Some(Action::Command(cmd.to_string())),
        "show-messages" | "showmsgs" => Some(Action::Command(cmd.to_string())),
        "clock-mode" => Some(Action::Command(cmd.to_string())),
        "command-prompt" => Some(Action::Command(cmd.to_string())),
        "has-session" | "has" => Some(Action::Command(cmd.to_string())),
        "move-window" | "movew" => Some(Action::Command(cmd.to_string())),
        "swap-window" | "swapw" => Some(Action::Command(cmd.to_string())),
        "link-window" | "linkw" => Some(Action::Command(cmd.to_string())),
        "unlink-window" | "unlinkw" => Some(Action::Command(cmd.to_string())),
        "find-window" | "findw" => Some(Action::Command(cmd.to_string())),
        "move-pane" | "movep" => Some(Action::Command(cmd.to_string())),
        "join-pane" | "joinp" => Some(Action::Command(cmd.to_string())),
        "resize-window" | "resizew" => Some(Action::Command(cmd.to_string())),
        "run-shell" | "run" => Some(Action::Command(cmd.to_string())),
        "if-shell" | "if" => Some(Action::Command(cmd.to_string())),
        "wait-for" | "wait" => Some(Action::Command(cmd.to_string())),
        "set-environment" | "setenv" => Some(Action::Command(cmd.to_string())),
        "show-environment" | "showenv" => Some(Action::Command(cmd.to_string())),
        "set-hook" => Some(Action::Command(cmd.to_string())),
        "bind-key" | "bind" => Some(Action::Command(cmd.to_string())),
        "unbind-key" | "unbind" => Some(Action::Command(cmd.to_string())),
        "attach-session" | "attach" | "a" | "at" => Some(Action::Command(cmd.to_string())),
        "new-session" | "new" => Some(Action::Command(cmd.to_string())),
        "server-info" | "info" => Some(Action::Command(cmd.to_string())),
        "start-server" | "start" => Some(Action::Command(cmd.to_string())),
        "lock-client" | "lockc" => Some(Action::Command(cmd.to_string())),
        "lock-server" | "lock" => Some(Action::Command(cmd.to_string())),
        "lock-session" | "locks" => Some(Action::Command(cmd.to_string())),
        "refresh-client" | "refresh" => Some(Action::Command(cmd.to_string())),
        "suspend-client" | "suspendc" => Some(Action::Command(cmd.to_string())),
        "switch-client" | "switchc" => {
            // Check for -T flag to switch key table
            if let Some(pos) = parts.iter().position(|p| *p == "-T") {
                if let Some(table) = parts.get(pos + 1) {
                    Some(Action::SwitchTable(table.to_string()))
                } else {
                    Some(Action::Command(cmd.to_string()))
                }
            } else {
                Some(Action::Command(cmd.to_string()))
            }
        }
        _ => Some(Action::Command(cmd.to_string())),
    }
}

/// Format an Action back to a command string
pub fn format_action(action: &Action) -> String {
    match action {
        Action::DisplayPanes => "display-panes".to_string(),
        Action::NewWindow => "new-window".to_string(),
        Action::SplitHorizontal => "split-window -h".to_string(),
        Action::SplitVertical => "split-window -v".to_string(),
        Action::KillPane => "kill-pane".to_string(),
        Action::NextWindow => "next-window".to_string(),
        Action::PrevWindow => "previous-window".to_string(),
        Action::CopyMode => "copy-mode".to_string(),
        Action::Paste => "paste-buffer".to_string(),
        Action::Detach => "detach-client".to_string(),
        Action::RenameWindow => "rename-window".to_string(),
        Action::WindowChooser => "choose-window".to_string(),
        Action::ZoomPane => "resize-pane -Z".to_string(),
        Action::MoveFocus(dir) => {
            let flag = match dir {
                FocusDir::Up => "-U",
                FocusDir::Down => "-D",
                FocusDir::Left => "-L",
                FocusDir::Right => "-R",
            };
            format!("select-pane {}", flag)
        }
        Action::Command(cmd) => cmd.clone(),
        Action::CommandChain(cmds) => cmds.join(" \\; "),
        Action::SwitchTable(table) => format!("switch-client -T {}", table),
    }
}

/// Parse a command line string, respecting quoted arguments
pub fn parse_command_line(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_double_quotes = false;
    let mut in_single_quotes = false;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if in_single_quotes {
            // Inside single quotes: everything is literal (no escape processing)
            if c == '\'' {
                in_single_quotes = false;
            } else {
                current.push(c);
            }
        } else if c == '\\' && in_double_quotes {
            // Inside double quotes, recognise two escape sequences:
            //   \"  → literal double-quote
            //   \\  → literal backslash
            // All other backslashes are kept literal because psmux is a
            // Windows-native tool where backslash is the normal path
            // separator (e.g. "C:\Program Files\Git\bin\bash.exe").
            if i + 1 < chars.len() && chars[i + 1] == '"' {
                current.push('"');
                i += 1; // skip the quote
            } else if i + 1 < chars.len() && chars[i + 1] == '\\' {
                current.push('\\');
                i += 1; // skip the second backslash
            } else {
                current.push(c); // literal backslash
            }
        } else if c == '"' {
            in_double_quotes = !in_double_quotes;
        } else if c == '\'' && !in_double_quotes {
            in_single_quotes = true;
        } else if c.is_whitespace() && !in_double_quotes {
            if !current.is_empty() {
                args.push(current.clone());
                current.clear();
            }
        } else {
            current.push(c);
        }
        i += 1;
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

/// Parse a menu definition string into a Menu structure
pub fn parse_menu_definition(def: &str, x: Option<i16>, y: Option<i16>) -> Menu {
    let mut menu = Menu {
        title: String::new(),
        items: Vec::new(),
        selected: 0,
        x,
        y,
    };

    let parts: Vec<&str> = def.split_whitespace().collect();
    if parts.is_empty() {
        return menu;
    }

    let mut i = 0;
    while i < parts.len() {
        if parts[i] == "-T" {
            if let Some(title) = parts.get(i + 1) {
                menu.title = title.trim_matches('"').to_string();
                i += 2;
                continue;
            }
        }

        if let Some(name) = parts.get(i) {
            let name = name.trim_matches('"').to_string();
            if name.is_empty() || name == "-" {
                menu.items.push(MenuItem {
                    name: String::new(),
                    key: None,
                    command: String::new(),
                    is_separator: true,
                });
                i += 1;
            } else {
                let key = parts
                    .get(i + 1)
                    .and_then(|k| k.trim_matches('"').chars().next());
                let command = parts
                    .get(i + 2)
                    .map(|c| c.trim_matches('"').to_string())
                    .unwrap_or_default();
                menu.items.push(MenuItem {
                    name,
                    key,
                    command,
                    is_separator: false,
                });
                i += 3;
            }
        } else {
            break;
        }
    }

    if menu.items.is_empty() && !def.is_empty() {
        menu.title = "Menu".to_string();
        menu.items.push(MenuItem {
            name: def.to_string(),
            key: Some('1'),
            command: def.to_string(),
            is_separator: false,
        });
    }

    menu
}

/// Fire hooks for a given event
pub fn fire_hooks(app: &mut AppState, event: &str) {
    if let Some(commands) = app.hooks.get(event).cloned() {
        for cmd in commands {
            let _ = execute_command_string(app, &cmd);
        }
    }
}

/// Execute an Action (from key bindings)
pub fn execute_action(app: &mut AppState, action: &Action) -> io::Result<bool> {
    match action {
        Action::DisplayPanes => {
            let win = &app.windows[app.active_idx];
            let mut rects: Vec<(Vec<usize>, ratatui::prelude::Rect)> = Vec::new();
            compute_rects(&win.root, app.last_window_area, &mut rects);
            app.display_map.clear();
            for (i, (path, _)) in rects.into_iter().enumerate() {
                if i >= 10 {
                    break;
                }
                let digit = (i + app.pane_base_index) % 10;
                app.display_map.push((digit, path));
            }
            app.mode = Mode::PaneChooser {
                opened_at: Instant::now(),
            };
        }
        Action::MoveFocus(dir) => {
            let d = *dir;
            switch_with_copy_save(app, |app| {
                crate::input::move_focus(app, d);
            });
        }
        Action::NewWindow => {
            let pty_system = portable_pty::native_pty_system();
            create_window(&*pty_system, app, None, None, None)?;
        }
        Action::SplitHorizontal => {
            split_active(app, LayoutKind::Horizontal)?;
        }
        Action::SplitVertical => {
            split_active(app, LayoutKind::Vertical)?;
        }
        Action::KillPane => {
            kill_active_pane(app)?;
        }
        Action::NextWindow => {
            if !app.windows.is_empty() {
                switch_with_copy_save(app, |app| {
                    app.last_window_idx = app.active_idx;
                    app.active_idx = (app.active_idx + 1) % app.windows.len();
                });
            }
        }
        Action::PrevWindow => {
            if !app.windows.is_empty() {
                switch_with_copy_save(app, |app| {
                    app.last_window_idx = app.active_idx;
                    app.active_idx = (app.active_idx + app.windows.len() - 1) % app.windows.len();
                });
            }
        }
        Action::CopyMode => {
            enter_copy_mode(app);
        }
        Action::Paste => {
            paste_latest(app)?;
        }
        Action::Detach => {
            return Ok(true);
        }
        Action::RenameWindow => {
            app.mode = Mode::RenamePrompt {
                input: String::new(),
            };
        }
        Action::WindowChooser => {
            let tree = build_choose_tree(app);
            let selected = tree
                .iter()
                .position(|e| e.is_current_session && e.is_active_window && !e.is_session_header)
                .unwrap_or(0);
            app.mode = Mode::WindowChooser { selected, tree };
        }
        Action::ZoomPane => {
            toggle_zoom(app);
        }
        Action::Command(cmd) => {
            execute_command_string(app, cmd)?;
        }
        Action::CommandChain(cmds) => {
            for cmd in cmds {
                execute_command_string(app, cmd)?;
            }
        }
        Action::SwitchTable(table) => {
            app.current_key_table = Some(table.clone());
        }
    }
    Ok(false)
}

pub fn execute_command_prompt(app: &mut AppState) -> io::Result<()> {
    let cmdline = match &app.mode {
        Mode::CommandPrompt { input, .. } => input.clone(),
        _ => String::new(),
    };
    app.mode = Mode::Passthrough;
    let parts: Vec<&str> = cmdline.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(());
    }
    match parts[0] {
        // Commands that need local (embedded-mode) handling.
        // In server mode the client sends these via TCP directly, so
        // execute_command_prompt() is only reached in embedded mode.
        "new-window" | "neww" => {
            let pty_system = portable_pty::native_pty_system();
            create_window(&*pty_system, app, None, None, None)?;
        }
        "split-window" | "splitw" => {
            let kind = if parts.contains(&"-h") {
                LayoutKind::Horizontal
            } else {
                LayoutKind::Vertical
            };
            split_active(app, kind)?;
        }
        "kill-pane" | "killp" => {
            kill_active_pane(app)?;
        }
        "capture-pane" | "capturep" => {
            capture_active_pane(app)?;
        }
        "save-buffer" | "saveb" => {
            if let Some(file) = parts.get(1) {
                save_latest_buffer(app, file)?;
            }
        }
        "list-sessions" | "ls" => {
            println!("default");
        }
        "attach-session" | "attach" | "a" | "at" => {}
        "list-windows" | "lsw" => {
            let output = generate_list_windows(app);
            show_output_popup(app, "list-windows", output);
        }
        "list-panes" | "lsp" => {
            let output = generate_list_panes(app);
            show_output_popup(app, "list-panes", output);
        }
        "list-clients" | "lsc" => {
            let output = generate_list_clients(app);
            show_output_popup(app, "list-clients", output);
        }
        "list-commands" | "lscm" => {
            let output = generate_list_commands();
            show_output_popup(app, "list-commands", output);
        }
        "show-hooks" => {
            let output = generate_show_hooks(app);
            show_output_popup(app, "show-hooks", output);
        }
        "next-window" => {
            app.last_window_idx = app.active_idx;
            app.active_idx = (app.active_idx + 1) % app.windows.len();
        }
        "previous-window" => {
            app.last_window_idx = app.active_idx;
            app.active_idx = (app.active_idx + app.windows.len() - 1) % app.windows.len();
        }
        "select-window" => {
            if let Some(tidx) = parts
                .iter()
                .position(|p| *p == "-t")
                .and_then(|i| parts.get(i + 1))
            {
                if let Some(n) = parse_window_target(tidx) {
                    if n >= app.window_base_index {
                        let internal_idx = n - app.window_base_index;
                        if internal_idx < app.windows.len() {
                            app.last_window_idx = app.active_idx;
                            app.active_idx = internal_idx;
                        }
                    }
                }
            }
        }
        _ => {
            execute_command_string(app, &cmdline)?;
        }
    }
    Ok(())
}

/// Execute a command string (used by menus, hooks, confirm dialogs, etc.)
pub fn execute_command_string(app: &mut AppState, cmd: &str) -> io::Result<()> {
    // Split on \; or ; to support command chaining (issue #192)
    let sub_commands = crate::config::split_chained_commands_pub(cmd);
    if sub_commands.len() > 1 {
        for sub in &sub_commands {
            execute_command_string_single(app, sub)?;
        }
        return Ok(());
    }
    execute_command_string_single(app, cmd)
}

fn execute_command_string_single(app: &mut AppState, cmd: &str) -> io::Result<()> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(());
    }

    match parts[0] {
        "new-window" | "neww" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, "new-window\n", &app.session_key);
            }
        }
        "split-window" | "splitw" => {
            if let Some(port) = app.control_port {
                // Forward the full command string to preserve -c, -d, -p etc. flags
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "kill-pane" => {
            let _ = kill_active_pane(app);
        }
        "kill-window" | "killw" => {
            if app.windows.len() > 1 {
                let mut win = app.windows.remove(app.active_idx);
                kill_all_children(&mut win.root);
                if app.active_idx >= app.windows.len() {
                    app.active_idx = app.windows.len() - 1;
                }
            }
        }
        "next-window" | "next" => {
            if !app.windows.is_empty() {
                switch_with_copy_save(app, |app| {
                    app.last_window_idx = app.active_idx;
                    app.active_idx = (app.active_idx + 1) % app.windows.len();
                });
            }
        }
        "previous-window" | "prev" => {
            if !app.windows.is_empty() {
                switch_with_copy_save(app, |app| {
                    app.last_window_idx = app.active_idx;
                    app.active_idx = (app.active_idx + app.windows.len() - 1) % app.windows.len();
                });
            }
        }
        "last-window" | "last" => {
            if app.last_window_idx < app.windows.len() {
                switch_with_copy_save(app, |app| {
                    std::mem::swap(&mut app.active_idx, &mut app.last_window_idx);
                });
            }
        }
        "select-window" | "selectw" => {
            if let Some(t_pos) = parts.iter().position(|p| *p == "-t") {
                if let Some(t) = parts.get(t_pos + 1) {
                    if let Some(idx) = parse_window_target(t) {
                        if idx >= app.window_base_index {
                            let internal_idx = idx - app.window_base_index;
                            if internal_idx < app.windows.len() {
                                switch_with_copy_save(app, |app| {
                                    app.last_window_idx = app.active_idx;
                                    app.active_idx = internal_idx;
                                });
                            }
                        }
                    }
                }
            }
        }
        "select-pane" | "selectp" => {
            // Save/restore copy mode across pane switches (tmux parity #43)
            let is_last = parts.contains(&"-l");
            if is_last {
                switch_with_copy_save(app, |app| {
                    let win = &mut app.windows[app.active_idx];
                    if !app.last_pane_path.is_empty() {
                        let tmp = win.active_path.clone();
                        win.active_path = app.last_pane_path.clone();
                        app.last_pane_path = tmp;
                    }
                });
                return Ok(());
            }
            let dir = if parts.contains(&"-U") {
                FocusDir::Up
            } else if parts.contains(&"-D") {
                FocusDir::Down
            } else if parts.contains(&"-L") {
                FocusDir::Left
            } else if parts.contains(&"-R") {
                FocusDir::Right
            } else {
                return Ok(());
            };
            // Zoom-aware directional navigation (tmux parity #134):
            // If zoomed, check if there's a direct neighbor OR a wrap target.
            // If yes: cancel zoom and navigate to it.
            // If no (single-pane window): no-op — stay zoomed.
            if app.windows[app.active_idx].zoom_saved.is_some() {
                // Temporarily unzoom to compute real geometry
                let saved = app.windows[app.active_idx].zoom_saved.take();
                if let Some(ref s) = saved {
                    let win = &mut app.windows[app.active_idx];
                    for (p, sz) in s.iter() {
                        if let Some(Node::Split { sizes, .. }) =
                            crate::tree::get_split_mut(&mut win.root, p)
                        {
                            *sizes = sz.clone();
                        }
                    }
                }
                crate::tree::resize_all_panes(app);
                // Find direct neighbor only (no wrap when zoomed — tmux parity)
                let win = &app.windows[app.active_idx];
                let mut rects: Vec<(Vec<usize>, ratatui::layout::Rect)> = Vec::new();
                crate::tree::compute_rects(&win.root, app.last_window_area, &mut rects);
                let active_idx = rects.iter().position(|(path, _)| *path == win.active_path);
                let has_target = if let Some(ai) = active_idx {
                    let (_, arect) = &rects[ai];
                    crate::input::find_best_pane_in_direction(&rects, ai, arect, dir, &[], &[])
                        .or_else(|| {
                            crate::input::find_wrap_target(&rects, ai, arect, dir, &[], &[])
                        })
                        .is_some()
                } else {
                    false
                };
                if has_target {
                    // Cancel zoom (already unzoomed) and navigate
                    switch_with_copy_save(app, |app| {
                        let win = &app.windows[app.active_idx];
                        app.last_pane_path = win.active_path.clone();
                        crate::input::move_focus(app, dir);
                    });
                } else {
                    // No target (single-pane) — re-zoom (restore saved zoom state)
                    if let Some(s) = saved {
                        let win = &mut app.windows[app.active_idx];
                        for (p, sz) in s.iter() {
                            if let Some(Node::Split { sizes, .. }) =
                                crate::tree::get_split_mut(&mut win.root, p)
                            {
                                *sizes = sz.clone();
                            }
                        }
                        win.zoom_saved = Some(s);
                    }
                    crate::tree::resize_all_panes(app);
                }
            } else {
                switch_with_copy_save(app, |app| {
                    let win = &app.windows[app.active_idx];
                    app.last_pane_path = win.active_path.clone();
                    crate::input::move_focus(app, dir);
                });
            }
        }
        "last-pane" | "lastp" => {
            switch_with_copy_save(app, |app| {
                let win = &mut app.windows[app.active_idx];
                if !app.last_pane_path.is_empty() {
                    let tmp = win.active_path.clone();
                    win.active_path = app.last_pane_path.clone();
                    app.last_pane_path = tmp;
                }
            });
        }
        "rename-window" | "renamew" => {
            if let Some(name) = parts.get(1) {
                let win = &mut app.windows[app.active_idx];
                win.name = name.to_string();
            }
        }
        "list-windows" | "lsw" => {
            let output = generate_list_windows(app);
            show_output_popup(app, "list-windows", output);
        }
        "list-panes" | "lsp" => {
            let output = generate_list_panes(app);
            show_output_popup(app, "list-panes", output);
        }
        "list-clients" | "lsc" => {
            let output = generate_list_clients(app);
            show_output_popup(app, "list-clients", output);
        }
        "list-commands" | "lscm" => {
            let output = generate_list_commands();
            show_output_popup(app, "list-commands", output);
        }
        "show-hooks" => {
            let output = generate_show_hooks(app);
            show_output_popup(app, "show-hooks", output);
        }
        "zoom-pane" | "zoom" | "resizep -Z" => {
            toggle_zoom(app);
        }
        "copy-mode" => {
            enter_copy_mode(app);
        }
        "hints" => {
            crate::hints::enter_hints_mode(app);
        }
        "display-panes" | "displayp" => {
            let win = &app.windows[app.active_idx];
            let mut rects: Vec<(Vec<usize>, ratatui::layout::Rect)> = Vec::new();
            compute_rects(&win.root, app.last_window_area, &mut rects);
            app.display_map.clear();
            for (i, (path, _)) in rects.into_iter().enumerate() {
                if i >= 10 {
                    break;
                }
                let digit = (i + app.pane_base_index) % 10;
                app.display_map.push((digit, path));
            }
            app.mode = Mode::PaneChooser {
                opened_at: Instant::now(),
            };
        }
        "confirm-before" | "confirm" => {
            let rest = parts[1..].join(" ");
            app.mode = Mode::ConfirmMode {
                prompt: format!("Run '{}'?", rest),
                command: rest,
                input: String::new(),
            };
        }
        "display-menu" | "menu" => {
            let rest = parts[1..].join(" ");
            let menu = parse_menu_definition(&rest, None, None);
            if !menu.items.is_empty() {
                app.mode = Mode::MenuMode { menu };
            }
        }
        "display-popup" | "popup" => {
            // Parse -w width, -h height, -E close-on-exit, -d start-dir flags
            let mut width_spec = "80".to_string();
            let mut height_spec = "24".to_string();
            let mut start_dir: Option<String> = None;
            let close_on_exit = parts.contains(&"-E");
            let mut skip_indices = std::collections::HashSet::new();
            skip_indices.insert(0); // skip the command name itself
            let mut i = 1;
            while i < parts.len() {
                match parts[i] {
                    "-w" => {
                        if let Some(v) = parts.get(i + 1) {
                            width_spec = v.to_string();
                            skip_indices.insert(i);
                            skip_indices.insert(i + 1);
                            i += 1;
                        }
                    }
                    "-h" => {
                        if let Some(v) = parts.get(i + 1) {
                            height_spec = v.to_string();
                            skip_indices.insert(i);
                            skip_indices.insert(i + 1);
                            i += 1;
                        }
                    }
                    "-d" | "-c" => {
                        if let Some(v) = parts.get(i + 1) {
                            start_dir = Some(v.to_string());
                            skip_indices.insert(i);
                            skip_indices.insert(i + 1);
                            i += 1;
                        }
                    }
                    "-E" | "-K" => {
                        skip_indices.insert(i);
                    }
                    _ => {}
                }
                i += 1;
            }
            // Resolve percentage dimensions against terminal size (#154)
            let (term_w, term_h) = crossterm::terminal::size().unwrap_or((120, 40));
            let width = parse_popup_dim_local(&width_spec, term_w, 80);
            let height = parse_popup_dim_local(&height_spec, term_h, 24);
            // Collect remaining args as the command
            let rest: String = parts
                .iter()
                .enumerate()
                .filter(|(idx, _)| !skip_indices.contains(idx))
                .map(|(_, a)| *a)
                .collect::<Vec<&str>>()
                .join(" ");

            // Spawn popup as a real Pane via the popup module
            let pane_result = if !rest.is_empty() {
                crate::popup::create_popup_pane(
                    &rest,
                    start_dir.as_deref(),
                    height.saturating_sub(2),
                    width.saturating_sub(2),
                    app.next_pane_id,
                    "1", // session name not available in local mode
                    &app.environment,
                )
            } else {
                None
            };

            app.mode = Mode::PopupMode {
                command: rest,
                output: String::new(),
                process: None,
                width,
                height,
                close_on_exit,
                popup_pane: pane_result.map(Box::new),
                scroll_offset: 0,
            };
        }
        "resize-pane" | "resizep" => {
            if parts.contains(&"-Z") {
                toggle_zoom(app);
            } else {
                // Forward to server for actual resize
                if let Some(port) = app.control_port {
                    let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
                }
            }
        }
        "swap-pane" | "swapp" => {
            if let Some(port) = app.control_port {
                let dir = if parts.contains(&"-U") { "-U" } else { "-D" };
                let _ =
                    send_control_to_port(port, &format!("swap-pane {}\n", dir), &app.session_key);
            }
        }
        "rotate-window" | "rotatew" => {
            if let Some(port) = app.control_port {
                let flag = if parts.contains(&"-D") { "-D" } else { "" };
                let _ = send_control_to_port(
                    port,
                    &format!("rotate-window {}\n", flag),
                    &app.session_key,
                );
            }
        }
        "break-pane" | "breakp" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, "break-pane\n", &app.session_key);
            }
        }
        "respawn-pane" | "respawnp" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, "respawn-pane\n", &app.session_key);
            }
        }
        "toggle-sync" => {
            app.sync_input = !app.sync_input;
        }
        "set-option" | "set" | "set-window-option" | "setw" => {
            // Apply locally so chained commands see immediate effect
            crate::config::parse_config_line(app, cmd);
            // Forward to server for option handling
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "bind-key" | "bind" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "unbind-key" | "unbind" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "source-file" | "source" => {
            // Always apply locally first for immediate visual feedback,
            // then forward to server for authoritative state update.
            if let Some(path) = parts.get(1) {
                crate::config::source_file(app, path);
            }
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "send-keys" | "send" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "detach-client" | "detach" => {
            // handled by caller to set quit flag
        }
        "rename-session" => {
            if let Some(name) = parts.get(1) {
                app.session_name = name.to_string();
            }
        }
        "select-layout" | "selectl" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "next-layout" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, "next-layout\n", &app.session_key);
            }
        }
        "pipe-pane" | "pipep" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "choose-tree" | "choose-window" | "choose-session" => {
            let tree = build_choose_tree(app);
            let selected = tree
                .iter()
                .position(|e| e.is_current_session && e.is_active_window && !e.is_session_header)
                .unwrap_or(0);
            app.mode = Mode::WindowChooser { selected, tree };
        }
        "command-prompt" => {
            // Support -I initial_text, -p prompt (ignored), -1 (ignored)
            let initial = parts
                .windows(2)
                .find(|w| w[0] == "-I")
                .map(|w| w[1].to_string())
                .unwrap_or_default();
            app.mode = Mode::CommandPrompt {
                input: initial.clone(),
                cursor: initial.len(),
            };
        }
        "paste-buffer" | "pasteb" => {
            paste_latest(app)?;
        }
        "set-buffer" | "setb" => {
            if let Some(text) = parts.get(1) {
                app.paste_buffers.insert(0, text.to_string());
                if app.paste_buffers.len() > 10 {
                    app.paste_buffers.pop();
                }
            }
        }
        "delete-buffer" | "deleteb" => {
            if !app.paste_buffers.is_empty() {
                app.paste_buffers.remove(0);
            }
        }
        "list-buffers" | "lsb" => {
            let mut output = String::new();
            for (i, buf) in app.paste_buffers.iter().enumerate() {
                output.push_str(&format!(
                    "buffer{}: {} bytes: \"{}\"\n",
                    i,
                    buf.len(),
                    &buf.chars().take(50).collect::<String>()
                ));
            }
            if output.is_empty() {
                output.push_str("(no buffers)\n");
            }
            show_output_popup(app, "list-buffers", output);
        }
        "show-buffer" | "showb" => {
            if let Some(buf) = app.paste_buffers.first() {
                show_output_popup(app, "show-buffer", buf.clone());
            }
        }
        "choose-buffer" | "chooseb" => {
            // Enter buffer chooser mode
            app.mode = Mode::BufferChooser { selected: 0 };
        }
        "clear-history" | "clearhist" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, "clear-history\n", &app.session_key);
            }
        }
        "kill-session" | "kill-ses" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, "kill-session\n", &app.session_key);
            }
        }
        "kill-server" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, "kill-server\n", &app.session_key);
            }
        }
        "has-session" | "has" => {
            // In embedded mode we ARE the session; always succeeds
        }
        "capture-pane" | "capturep" => {
            capture_active_pane(app)?;
        }
        "save-buffer" | "saveb" => {
            if let Some(file) = parts.get(1) {
                save_latest_buffer(app, file)?;
            }
        }
        "load-buffer" | "loadb" => {
            if let Some(path) = parts.get(1) {
                if let Ok(data) = std::fs::read_to_string(path) {
                    app.paste_buffers.insert(0, data);
                    if app.paste_buffers.len() > 10 {
                        app.paste_buffers.pop();
                    }
                }
            }
        }
        "clock-mode" => {
            app.mode = Mode::ClockMode;
        }
        "list-sessions" | "ls" => {
            // Show all sessions from filesystem
            let output = crate::session::list_session_names().join("\n") + "\n";
            show_output_popup(app, "list-sessions", output);
        }
        "list-keys" | "lsk" => {
            let mut output = String::new();
            for (table_name, binds) in &app.key_tables {
                for bind in binds {
                    let key_str = crate::config::format_key_binding(&bind.key);
                    let cmd_str = format_action(&bind.action);
                    output.push_str(&format!(
                        "bind-key -T {} {} {}\n",
                        table_name, key_str, cmd_str
                    ));
                }
            }
            if output.is_empty() {
                output.push_str("(no bindings)\n");
            }
            show_output_popup(app, "list-keys", output);
        }
        "show-options" | "show" | "show-window-options" | "showw" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "display-message" | "display" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "show-messages" | "showmsgs" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "set-environment" | "setenv" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "show-environment" | "showenv" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "set-hook" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "send-prefix" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, "send-prefix\n", &app.session_key);
            }
        }
        "if-shell" | "if" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "wait-for" | "wait" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "find-window" | "findw" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "move-window" | "movew" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "swap-window" | "swapw" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "link-window" | "linkw" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "unlink-window" | "unlinkw" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "move-pane" | "movep" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "join-pane" | "joinp" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "resize-window" | "resizew" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "respawn-window" | "respawnw" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
        "previous-layout" | "prevl" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, "previous-layout\n", &app.session_key);
            }
        }
        "attach-session" | "attach" | "a" | "at" => {
            // Already attached in a running session; no-op
        }
        "start-server" | "start" => {
            // Already running
        }
        "server-info" | "info" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, "server-info\n", &app.session_key);
            }
        }
        "new-session" | "new" => {
            // Cannot create a session from inside a session; show feedback
            show_output_popup(
                app,
                "new-session",
                "(cannot create a new session from inside a session)\n".to_string(),
            );
        }
        "lock-client" | "lockc" | "lock-server" | "lock" | "lock-session" | "locks" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, "lock-server\n", &app.session_key);
            }
        }
        "refresh-client" | "refresh" => {
            // Trigger a redraw; no explicit action needed in embedded mode
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, "refresh-client\n", &app.session_key);
            }
        }
        "suspend-client" | "suspendc" => {
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, "suspend-client\n", &app.session_key);
            }
        }
        "choose-client" => {
            // Single-client model; no-op
        }
        "customize-mode" => {
            // tmux 3.2+ customize-mode; stub for compatibility
        }
        "run-shell" | "run" => {
            // Parse with quote-aware parser to handle nested quotes properly
            let args = parse_command_line(cmd);
            let mut cmd_parts: Vec<&str> = Vec::new();
            let mut background = false;
            for arg in &args[1..] {
                if arg == "-b" {
                    background = true;
                } else {
                    cmd_parts.push(arg);
                }
            }
            let shell_cmd = cmd_parts.join(" ");
            if !shell_cmd.is_empty() {
                // Expand ~ to home directory + XDG fallback for plugin paths
                let shell_cmd = crate::util::expand_run_shell_path(&shell_cmd);
                // Set PSMUX_TARGET_SESSION so child scripts connect to the correct server
                let target_session = app.port_file_base();

                if background {
                    // -b flag: fire and forget, no output capture
                    let mut c = build_run_shell_command(&shell_cmd);
                    if !target_session.is_empty() {
                        c.env("PSMUX_TARGET_SESSION", &target_session);
                    }
                    let _ = c.spawn();
                } else {
                    // No -b: spawn async to avoid blocking the UI thread.
                    // Interactive commands (htop, vim, etc.) would freeze psmux
                    // if we used synchronous .output() on the main thread.
                    // Lazily create the channel pair on first use.
                    if app.run_shell_tx.is_none() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        app.run_shell_tx = Some(tx);
                        app.run_shell_rx = Some(rx);
                    }
                    let tx = app.run_shell_tx.as_ref().unwrap().clone();
                    let shell_cmd = shell_cmd.clone();
                    let shell_cmd_display = shell_cmd.clone();
                    let target_session = target_session.clone();
                    std::thread::spawn(move || {
                        let mut c = build_run_shell_command(&shell_cmd);
                        if !target_session.is_empty() {
                            c.env("PSMUX_TARGET_SESSION", &target_session);
                        }
                        // Detach stdin so interactive programs exit immediately
                        c.stdin(std::process::Stdio::null());
                        match c.output() {
                            Ok(output) => {
                                let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
                                let stderr = String::from_utf8_lossy(&output.stderr);
                                if !stderr.is_empty() {
                                    if !text.is_empty() && !text.ends_with('\n') {
                                        text.push('\n');
                                    }
                                    text.push_str(&stderr);
                                }
                                // Send result back; empty output is also sent so
                                // the status message "running..." can be cleared.
                                let _ = tx.send(("run-shell".to_string(), text));
                            }
                            Err(e) => {
                                let _ =
                                    tx.send(("run-shell".to_string(), format!("run-shell: {}", e)));
                            }
                        }
                    });
                    app.status_message =
                        Some((format!("running: {}", shell_cmd_display), Instant::now()));
                }
            }
        }
        _ => {
            // Apply config locally (handles set, bind, source, etc.)
            let old_shell = app.default_shell.clone();
            crate::config::parse_config_line(app, cmd);
            if app.default_shell != old_shell {
                if let Some(mut wp) = app.warm_pane.take() {
                    wp.child.kill().ok();
                }
            }
            // Also forward unknown commands to server (catch-all for tmux compat)
            if let Some(port) = app.control_port {
                let _ = send_control_to_port(port, &format!("{}\n", cmd), &app.session_key);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests-rs/test_commands.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests-rs/test_commands_new.rs"]
mod tests_new_commands;

#[cfg(test)]
#[path = "../tests-rs/test_issue192_command_chaining.rs"]
mod tests_issue192_command_chaining;
