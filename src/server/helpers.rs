use std::io;

use crate::format::expand_format_for_window;
use crate::types::{AppState, Node, Window};
use crate::util::WinInfo;

/// Collect all leaf pane paths in tree order (for next/prev pane cycling).
pub(crate) fn collect_pane_paths_server(
    node: &Node,
    path: &mut Vec<usize>,
    panes: &mut Vec<Vec<usize>>,
) {
    match node {
        Node::Leaf(_) => {
            panes.push(path.clone());
        }
        Node::Split { children, .. } => {
            for (i, c) in children.iter().enumerate() {
                path.push(i);
                collect_pane_paths_server(c, path, panes);
                path.pop();
            }
        }
    }
}

/// Serialize key_tables into a compact JSON array for syncing to the client.
/// Format: [{"t":"prefix","k":"x","c":"split-window -v","r":false}, ...]
pub(crate) fn serialize_bindings_json(app: &AppState) -> String {
    use crate::commands::format_action;
    use crate::config::format_key_binding;
    let mut out = String::from("[");
    let mut first = true;
    for (table_name, binds) in &app.key_tables {
        for bind in binds {
            if !first {
                out.push(',');
            }
            first = false;
            let key_str = json_escape_string(&format_key_binding(&bind.key));
            let cmd_str = json_escape_string(&format_action(&bind.action));
            let tbl_str = json_escape_string(table_name);
            out.push_str(&format!(
                "{{\"t\":\"{}\",\"k\":\"{}\",\"c\":\"{}\",\"r\":{}}}",
                tbl_str, key_str, cmd_str, bind.repeat
            ));
        }
    }
    out.push(']');
    out
}

/// Escape a string for embedding inside a JSON double-quoted value.
/// Handles backslashes, double-quotes, and control characters.
pub(crate) fn json_escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Build windows JSON with pre-expanded tab_text for each window.
/// The tab_text is the fully expanded window-status-format / window-status-current-format.
pub(crate) fn list_windows_json_with_tabs(app: &AppState) -> io::Result<String> {
    let mut v: Vec<WinInfo> = Vec::new();
    for (i, w) in app.windows.iter().enumerate() {
        let is_active = i == app.active_idx;
        let fmt = if is_active {
            &app.window_status_current_format
        } else {
            &app.window_status_format
        };
        let tab = expand_format_for_window(fmt, app, i);
        v.push(WinInfo {
            id: w.id,
            name: w.name.clone(),
            active: is_active,
            activity: w.activity_flag,
            tab_text: tab,
        });
    }
    serde_json::to_string(&v).map_err(|e| io::Error::other(format!("json error: {e}")))
}

/// Sum data_version counters across all panes in the active window.
pub(crate) fn combined_data_version(app: &AppState) -> u64 {
    let mut v = 0u64;
    fn walk(node: &Node, v: &mut u64) {
        match node {
            Node::Leaf(p) => {
                *v = v.wrapping_add(p.data_version.load(std::sync::atomic::Ordering::Acquire));
            }
            Node::Split { children, .. } => {
                for c in children {
                    walk(c, v);
                }
            }
        }
    }
    if let Some(win) = app.windows.get(app.active_idx) {
        walk(&win.root, &mut v);
    }
    // Include mode discriminant so overlay state changes (PopupMode, MenuMode,
    // ConfirmMode, PaneChooser, ClockMode) always invalidate the cached version.
    // Without this, the NC optimization could return stale frames that lack
    // overlay fields, causing overlays to not render on the client.
    let mode_tag: u64 = match &app.mode {
        crate::types::Mode::Passthrough => 0,
        crate::types::Mode::Prefix { .. } => 1,
        crate::types::Mode::CopyMode => 2,
        crate::types::Mode::CopySearch { .. } => 3,
        crate::types::Mode::ClockMode => 4,
        crate::types::Mode::PopupMode { .. } => 5,
        crate::types::Mode::ConfirmMode { .. } => 6,
        crate::types::Mode::MenuMode { .. } => 7,
        crate::types::Mode::PaneChooser { .. } => 8,
        crate::types::Mode::BufferChooser { .. } => 9,
        _ => 10,
    };
    v = v.wrapping_add(mode_tag.wrapping_mul(0x1_0000_0000));
    v
}

/// Per-window data version for activity detection
pub(crate) fn window_data_version(win: &Window) -> u64 {
    let mut v = 0u64;
    fn walk(node: &Node, v: &mut u64) {
        match node {
            Node::Leaf(p) => {
                *v = v.wrapping_add(p.data_version.load(std::sync::atomic::Ordering::Acquire));
            }
            Node::Split { children, .. } => {
                for c in children {
                    walk(c, v);
                }
            }
        }
    }
    walk(&win.root, &mut v);
    v
}

/// Check non-active windows for output activity and set their activity_flag
pub(crate) fn check_window_activity(app: &mut AppState) {
    if !app.monitor_activity {
        return;
    }
    let active = app.active_idx;
    for (i, win) in app.windows.iter_mut().enumerate() {
        if i == active {
            // Active window: clear flag, update version
            win.activity_flag = false;
            win.last_seen_version = window_data_version(win);
            continue;
        }
        let cur = window_data_version(win);
        if cur != win.last_seen_version {
            win.activity_flag = true;
            win.last_seen_version = cur;
        }
    }
}

/// Complete list of supported tmux-compatible commands (for list-commands).
pub(crate) const TMUX_COMMANDS: &[&str] = &[
    "attach-session (attach)",
    "bind-key (bind)",
    "break-pane (breakp)",
    "capture-pane (capturep)",
    "choose-buffer",
    "choose-client",
    "choose-session",
    "choose-tree",
    "choose-window",
    "clear-history (clearhist)",
    "clock-mode",
    "command-prompt",
    "confirm-before (confirm)",
    "copy-mode",
    "customize-mode",
    "delete-buffer (deleteb)",
    "detach-client (detach)",
    "display-menu (menu)",
    "display-message (display)",
    "display-panes (displayp)",
    "display-popup (popup)",
    "find-window (findw)",
    "has-session (has)",
    "if-shell (if)",
    "join-pane (joinp)",
    "kill-pane (killp)",
    "kill-server",
    "kill-session",
    "kill-window (killw)",
    "last-pane (lastp)",
    "last-window (last)",
    "link-window (linkw)",
    "list-buffers (lsb)",
    "list-clients (lsc)",
    "list-commands (lscm)",
    "list-keys (lsk)",
    "list-panes (lsp)",
    "list-sessions (ls)",
    "list-windows (lsw)",
    "load-buffer (loadb)",
    "lock-client (lockc)",
    "lock-server (lock)",
    "lock-session (locks)",
    "move-pane (movep)",
    "move-window (movew)",
    "new-session (new)",
    "new-window (neww)",
    "next-layout (nextl)",
    "next-window (next)",
    "paste-buffer (pasteb)",
    "pipe-pane (pipep)",
    "previous-layout (prevl)",
    "previous-window (prev)",
    "refresh-client (refresh)",
    "rename-session (rename)",
    "rename-window (renamew)",
    "resize-pane (resizep)",
    "resize-window (resizew)",
    "respawn-pane (respawnp)",
    "respawn-window (respawnw)",
    "rotate-window (rotatew)",
    "run (run-command)",
    "run-shell",
    "save-buffer (saveb)",
    "select-layout (selectl)",
    "select-pane (selectp)",
    "select-window (selectw)",
    "send-keys (send)",
    "send-prefix",
    "server-info (info)",
    "set-buffer (setb)",
    "set-environment (setenv)",
    "set-hook",
    "set-option (set)",
    "set-window-option (setw)",
    "show-buffer (showb)",
    "show-environment (showenv)",
    "show-hooks",
    "show-messages (showmsgs)",
    "show-options (show)",
    "show-window-options (showw)",
    "source-file (source)",
    "split-window (splitw)",
    "start-server (start)",
    "suspend-client (suspendc)",
    "swap-pane (swapp)",
    "swap-window (swapw)",
    "switch-client (switchc)",
    "unbind-key (unbind)",
    "unlink-window (unlinkw)",
    "wait-for (wait)",
];
