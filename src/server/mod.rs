mod connection;
pub(crate) mod helpers;
mod options;

use std::env;
use std::io::{self, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::native_pty_system;
use ratatui::prelude::Rect;

use crate::pane::{
    create_window, create_window_raw, kill_active_pane, kill_pane_by_id, spawn_warm_pane,
    split_active_with_command,
};
use crate::platform::install_console_ctrl_handler;
use crate::tree::{
    self, active_pane, active_pane_mut, find_window_index_by_id, focus_pane_by_id,
    focus_pane_by_index, get_active_pane_id, get_split_mut, kill_all_children, path_exists,
    resize_all_panes,
};
use crate::types::{
    Action, AppState, Bind, CtrlReq, FocusDir, LayoutKind, Mode, Node, PipePaneState, WaitChannel,
    WaitForOp, VERSION,
};

use helpers::{
    collect_pane_paths_server, combined_data_version, json_escape_string,
    list_windows_json_with_tabs, serialize_bindings_json, TMUX_COMMANDS,
};
use options::{apply_set_option, get_option_value, get_window_option_value, render_window_options};

use crate::commands::{
    execute_command_string, format_action, parse_command_to_action, parse_menu_definition,
};
use crate::config::{
    format_key_binding, load_config, normalize_key_for_binding, parse_config_content,
    parse_key_string,
};
use crate::copy_mode::{
    capture_active_pane_range, capture_active_pane_styled, capture_active_pane_text,
    capture_active_pane_text_clean, current_prompt_pos, enter_copy_mode, exit_copy_mode,
    move_copy_cursor, scroll_copy_down, scroll_copy_up, switch_with_copy_save, yank_selection,
};
use crate::format::{
    expand_format, format_list_panes, format_list_windows, set_buffer_idx_override,
};
use crate::help;
use crate::input::{
    find_best_pane_in_direction, find_wrap_target, move_focus, send_key_to_active,
    send_paste_to_active, send_text_to_active,
};
use crate::layout::{
    apply_layout, cycle_layout, cycle_layout_reverse, dump_layout_json, dump_layout_json_fast,
};
use crate::util::{base64_encode, list_tree_json, list_windows_json, list_windows_tmux};
use crate::window_ops::{
    break_pane_to_window, remote_mouse_button, remote_mouse_down, remote_mouse_drag,
    remote_mouse_motion, remote_mouse_up, remote_scroll_down, remote_scroll_up,
    resize_pane_absolute, resize_pane_horizontal, resize_pane_vertical, respawn_active_pane,
    rotate_panes, swap_pane, toggle_zoom, unzoom_if_zoomed,
};

/// Build a JSON fragment with overlay state (popup, menu, confirm, display_panes).
/// Returns a string like `,"popup_active":true,"popup_command":"...","popup_lines":[...]`
/// that can be injected into the dump-state JSON before the closing `}`.
fn serialize_overlay_json(app: &AppState) -> String {
    use crate::server::helpers::json_escape_string;
    let mut out = String::new();
    match &app.mode {
        Mode::PopupMode {
            command,
            output,
            width,
            height,
            popup_pane,
            ..
        } => {
            out.push_str(",\"popup_active\":true");
            out.push_str(",\"popup_command\":\"");
            out.push_str(&json_escape_string(command));
            out.push('"');
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!(",\"popup_width\":{},\"popup_height\":{}", width, height),
            );
            let inner_h = height.saturating_sub(2);
            let inner_w = width.saturating_sub(2);

            if let Some(pane) = popup_pane {
                // PTY popup: serialize using the shared pane screen serializer
                out.push_str(",\"popup_rows\":[");
                if let Ok(parser) = pane.term.lock() {
                    let screen = parser.screen();
                    let rows_data = crate::layout::serialize_screen_rows(screen, inner_h, inner_w);
                    for (i, row) in rows_data.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        out.push_str("{\"runs\":[");
                        for (j, run) in row.runs.iter().enumerate() {
                            if j > 0 {
                                out.push(',');
                            }
                            out.push_str("{\"text\":\"");
                            crate::popup::json_esc_inline(&run.text, &mut out);
                            out.push_str("\",\"fg\":\"");
                            out.push_str(&run.fg);
                            out.push_str("\",\"bg\":\"");
                            out.push_str(&run.bg);
                            let _ = std::fmt::Write::write_fmt(
                                &mut out,
                                format_args!(
                                    "\",\"flags\":{},\"width\":{}}}",
                                    run.flags, run.width
                                ),
                            );
                        }
                        out.push_str("]}");
                    }
                }
                out.push(']');
                out.push_str(",\"popup_lines\":[]");
                out.push_str(",\"popup_has_pty\":true");
            } else {
                // Static (non-PTY) popup: plain text lines
                out.push_str(",\"popup_rows\":[]");
                out.push_str(",\"popup_lines\":[");
                for (i, line) in output.lines().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push('"');
                    out.push_str(&json_escape_string(line));
                    out.push('"');
                }
                out.push(']');
                out.push_str(",\"popup_has_pty\":false");
            }
        }
        Mode::ConfirmMode { prompt, .. } => {
            out.push_str(",\"confirm_active\":true,\"confirm_prompt\":\"");
            out.push_str(&json_escape_string(prompt));
            out.push('"');
        }
        Mode::MenuMode { menu } => {
            out.push_str(",\"menu_active\":true,\"menu_title\":\"");
            out.push_str(&json_escape_string(&menu.title));
            out.push('"');
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!(",\"menu_selected\":{}", menu.selected),
            );
            out.push_str(",\"menu_items\":[");
            for (i, item) in menu.items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if item.is_separator {
                    out.push_str("{\"sep\":true}");
                } else {
                    out.push_str("{\"name\":\"");
                    out.push_str(&json_escape_string(&item.name));
                    out.push_str("\",\"key\":");
                    if let Some(k) = item.key {
                        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("\"{}\"", k));
                    } else {
                        out.push_str("null");
                    }
                    out.push('}');
                }
            }
            out.push(']');
        }
        Mode::HintsMode(ref state) => {
            use crate::server::helpers::json_escape_string;
            out.push_str(",\"hints_active\":true,\"hints_input\":\"");
            out.push_str(&json_escape_string(&state.input));
            out.push_str("\",\"hints\":[");
            for (i, m) in state.matches.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let _ = std::fmt::Write::write_fmt(
                    &mut out,
                    format_args!(
                        r#"{{"row":{},"start_col":{},"end_col":{},"label":"{}","text":"{}"}}"#,
                        m.row,
                        m.start_col,
                        m.end_col,
                        json_escape_string(&m.label),
                        json_escape_string(&m.text)
                    ),
                );
            }
            out.push(']');
        }
        Mode::PaneChooser { .. } => {
            out.push_str(",\"display_panes\":true");
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!(",\"pane_base_index\":{}", app.pane_base_index),
            );
        }
        _ => {}
    }
    // Include status_message for display-message without -p (#110)
    if let Some((ref msg, since)) = app.status_message {
        let elapsed = since.elapsed().as_millis() as u64;
        let display_time = app.display_time_ms;
        if elapsed < display_time {
            out.push_str(",\"status_message\":\"");
            out.push_str(&json_escape_string(msg));
            out.push('"');
        }
    }
    out
}

fn should_spawn_warm_server(app: &AppState) -> bool {
    app.warm_enabled && !is_warm_server(app) && !app.destroy_unattached
}

/// Returns true when this server instance is a warm (standby) server.
/// Warm servers are internal implementation details that should always
/// clean up when their last pane exits, regardless of user config
/// settings like `exit-empty` or `remain-on-exit`.
fn is_warm_server(app: &AppState) -> bool {
    crate::session::is_warm_session(&app.port_file_base())
}

/// Returns true when every pane across all windows has exited (is dead).
/// This is needed for warm server cleanup when `remain-on-exit` is on:
/// `remain-on-exit` keeps dead panes in the tree (preventing window
/// removal), so we must explicitly check whether any pane is still alive.
fn all_panes_dead(app: &mut AppState) -> bool {
    fn node_all_dead(node: &mut Node) -> bool {
        match node {
            Node::Leaf(p) => p.dead || matches!(p.child.try_wait(), Ok(Some(_))),
            Node::Split { children, .. } => children.iter_mut().all(node_all_dead),
        }
    }
    app.windows.iter_mut().all(|w| node_all_dead(&mut w.root))
}

/// Check all wait-pane waiters and notify any whose pane has exited.
/// Waiters for panes that no longer exist in the tree are also notified
/// with exit code 0 (pane was already reaped).
fn drain_wait_pane_queue(app: &mut AppState) {
    app.wait_pane_queue.retain(|(pane_id, sender)| {
        // Search for the pane across all windows
        let mut found = false;
        for win in app.windows.iter_mut() {
            if let Some(path) = crate::tree::find_path_by_id(&win.root, *pane_id) {
                found = true;
                if let Some(p) = crate::tree::active_pane_mut(&mut win.root, &path) {
                    if p.dead {
                        let code = p
                            .child
                            .try_wait()
                            .ok()
                            .flatten()
                            .map_or(0, |s| s.exit_code() as i32);
                        let _ = sender.send(code);
                        return false; // remove from queue
                    }
                    if let Ok(Some(status)) = p.child.try_wait() {
                        let _ = sender.send(status.exit_code() as i32);
                        return false; // remove from queue
                    }
                }
                break;
            }
        }
        if !found {
            // Pane was already reaped — respond with exit code 0
            let _ = sender.send(0);
            return false;
        }
        true // keep waiting
    });
}

/// Spawn a single warm server process with the given session name.
/// Returns true if a new server was spawned, false if one already existed.
fn spawn_one_warm_server(app: &AppState, warm_session_name: &str) -> bool {
    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .unwrap_or_default();
    let warm_base = if let Some(ref sn) = app.socket_name {
        format!("{}__{}", sn, warm_session_name)
    } else {
        warm_session_name.to_string()
    };
    let warm_port_path = format!("{}\\.psmux\\{}.port", home, warm_base);
    if std::path::Path::new(&warm_port_path).exists() {
        // Check if it's actually alive
        if let Ok(port_str) = std::fs::read_to_string(&warm_port_path) {
            if let Ok(port) = port_str.trim().parse::<u16>() {
                let addr = format!("127.0.0.1:{}", port);
                if std::net::TcpStream::connect_timeout(
                    &addr.parse().unwrap(),
                    Duration::from_millis(100),
                )
                .is_ok()
                {
                    return false; // warm server already running
                }
            }
        }
        // Stale port file — remove it
        let _ = std::fs::remove_file(&warm_port_path);
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("psmux"));
    let mut args: Vec<String> = vec!["server".into(), "-s".into(), warm_session_name.to_string()];
    if let Some(ref sn) = app.socket_name {
        args.push("-L".into());
        args.push(sn.clone());
    }
    // Pass current terminal dimensions so the warm server's first window
    // and warm pane are spawned at the right size.
    let area = app.last_window_area;
    if area.width > 1 && area.height > 1 {
        args.push("-x".into());
        args.push(area.width.to_string());
        args.push("-y".into());
        args.push(area.height.to_string());
    }
    #[cfg(windows)]
    {
        let _ = crate::platform::spawn_server_hidden(&exe, &args);
    }
    #[cfg(not(windows))]
    {
        let mut cmd = std::process::Command::new(&exe);
        for a in &args {
            cmd.arg(a);
        }
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
        let _ = cmd.spawn();
    }
    true
}

/// Spawn standby "warm server" processes up to the configured pool size.
/// When `warm-pool-size` is 0, no warm servers are spawned.
/// When >= 1, uses session names `__warm__` (for size=1) or
/// `__warm__0`, `__warm__1`, ... (for size>1).
/// After a warm server is claimed, call this again to replenish the pool.
fn spawn_warm_server(app: &AppState) {
    if !should_spawn_warm_server(app) {
        return;
    }
    let pool_size = app.warm_pool_size;
    if pool_size == 0 {
        return;
    }
    if pool_size == 1 {
        // Legacy single-server path: use `__warm__` for backwards compat
        spawn_one_warm_server(app, "__warm__");
    } else {
        // Multi-server pool: use `__warm__0`, `__warm__1`, ...
        for i in 0..pool_size {
            spawn_one_warm_server(app, &format!("__warm__{}", i));
        }
    }
}

/// Extract the Win32 process ID from a pane by its numeric ID.
/// Returns None if the pane is not found or the PID cannot be read.
fn get_pane_process_id(app: &AppState, pane_id: usize) -> Option<u32> {
    for win in &app.windows {
        if let Some(path) = crate::tree::find_path_by_id(&win.root, pane_id) {
            if let Some(p) = crate::tree::active_pane(&win.root, &path) {
                return p.child.process_id();
            }
        }
    }
    None
}

/// Read the current screen buffer of a pane as plaintext (newline-joined rows).
/// `pane_id = None` targets the active pane of the active window.
/// Returns an empty string if the pane is not found or the term lock is poisoned.
fn read_pane_contents(app: &AppState, pane_id: Option<usize>) -> String {
    let pane_opt: Option<&crate::types::Pane> = match pane_id {
        Some(id) => app.windows.iter().find_map(|win| {
            crate::tree::find_path_by_id(&win.root, id)
                .and_then(|p| crate::tree::active_pane(&win.root, &p))
        }),
        None => app
            .windows
            .get(app.active_idx)
            .and_then(|win| crate::tree::active_pane(&win.root, &win.active_path)),
    };
    let Some(p) = pane_opt else {
        return String::new();
    };
    let Ok(parser) = p.term.lock() else {
        return String::new();
    };
    let screen = parser.screen();
    let mut text = String::new();
    for r in 0..p.last_rows {
        let mut row = String::new();
        for c in 0..p.last_cols {
            if let Some(cell) = screen.cell(r, c) {
                row.push_str(cell.contents());
            } else {
                row.push(' ');
            }
        }
        text.push_str(row.trim_end());
        text.push('\n');
    }
    text
}

/// Parse a popup dimension spec: "80" (absolute) or "95%" (percentage of term_dim).
fn parse_popup_dim(spec: &str, term_dim: u16, default: u16) -> u16 {
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

/// Compute the effective display size from all connected clients' terminal sizes.
/// Returns None if no clients have reported sizes.
fn compute_effective_client_size(app: &AppState) -> Option<(u16, u16)> {
    if app.client_sizes.is_empty() {
        return None;
    }
    match app.window_size.as_str() {
        "smallest" => Some((
            app.client_sizes.values().map(|s| s.0).min().unwrap(),
            app.client_sizes.values().map(|s| s.1).min().unwrap(),
        )),
        "largest" => Some((
            app.client_sizes.values().map(|s| s.0).max().unwrap(),
            app.client_sizes.values().map(|s| s.1).max().unwrap(),
        )),
        _ => {
            // "latest" — use latest client's size, fall back to smallest
            if let Some(cid) = app.latest_client_id {
                if let Some(&size) = app.client_sizes.get(&cid) {
                    return Some(size);
                }
            }
            Some((
                app.client_sizes.values().map(|s| s.0).min().unwrap(),
                app.client_sizes.values().map(|s| s.1).min().unwrap(),
            ))
        }
    }
}

/// Process a single CtrlReq during the post-config plugin drain loop.
/// Handles the subset of requests that plugin scripts send (set, show, bind,
/// source-file) and silently drops others.
fn drain_plugin_req(
    app: &mut AppState,
    req: CtrlReq,
    shared_aliases: &std::sync::Arc<std::sync::RwLock<std::collections::HashMap<String, String>>>,
) {
    match req {
        CtrlReq::SetOption(option, value) => {
            apply_set_option(app, &option, &value, false);
            if option == "command-alias" {
                if let Ok(mut map) = shared_aliases.write() {
                    *map = app.command_aliases.clone();
                }
            }
        }
        CtrlReq::SetOptionQuiet(option, value, quiet, only_if_unset) => {
            if only_if_unset && app.user_set_options.contains(&option) {
                // -o: no-op when the option was already set by the user
            } else {
                apply_set_option(app, &option, &value, quiet);
                app.user_set_options.insert(option.clone());
            }
            if option == "command-alias" {
                if let Ok(mut map) = shared_aliases.write() {
                    *map = app.command_aliases.clone();
                }
            }
        }
        CtrlReq::SetOptionAppend(option, value) => {
            if option.starts_with('@') {
                let existing = app.user_options.get(&option).cloned().unwrap_or_default();
                app.user_options
                    .insert(option, format!("{}{}", existing, value));
            } else {
                match option.as_str() {
                    "status-left" => app.status_left.push_str(&value),
                    "status-right" => app.status_right.push_str(&value),
                    "status-style" => app.status_style.push_str(&value),
                    _ => {}
                }
            }
        }
        CtrlReq::SetOptionUnset(option) => {
            app.user_set_options.remove(&option);
            if option.starts_with('@') {
                app.user_options.remove(&option);
            }
        }
        CtrlReq::ShowOptionValue(resp, name) => {
            let val = get_option_value(app, &name);
            let _ = resp.send(val);
        }
        CtrlReq::ShowWindowOptionValue(resp, name) => {
            let val = get_window_option_value(app, &name);
            let _ = resp.send(val);
        }
        CtrlReq::ShowOptions(resp) => {
            // Minimal: just send empty to unblock the caller
            let _ = resp.send(String::new());
        }
        CtrlReq::ShowWindowOptions(resp) => {
            let _ = resp.send(render_window_options(app));
        }
        CtrlReq::BindKey(table_name, key, command, repeat) => {
            if let Some(kc) = parse_key_string(&key) {
                let kc = normalize_key_for_binding(kc);
                let sub_cmds = crate::config::split_chained_commands_pub(&command);
                let action = if sub_cmds.len() > 1 {
                    Some(Action::CommandChain(sub_cmds))
                } else {
                    parse_command_to_action(&command)
                };
                if let Some(act) = action {
                    let table = app.key_tables.entry(table_name).or_default();
                    table.retain(|b| b.key != kc);
                    table.push(Bind {
                        key: kc,
                        action: act,
                        repeat,
                    });
                }
            }
        }
        CtrlReq::SourceFile(path) => {
            app.defaults_suppressed = false;
            crate::config::source_file(app, &path);
        }
        // Ignore other request types during plugin drain
        _ => {}
    }
}

pub fn run_server(
    session_name: String,
    socket_name: Option<String>,
    initial_command: Option<String>,
    raw_command: Option<Vec<String>>,
    start_dir: Option<String>,
    window_name: Option<String>,
    init_size: Option<(u16, u16)>,
) -> io::Result<()> {
    // Keep only the newest 20 crash reports across runs.
    crate::crash::prune_crashes(20);
    // Install console control handler to prevent termination on client detach
    install_console_ctrl_handler();

    let pty_system = native_pty_system();

    let mut app = AppState::new(session_name);
    app.socket_name = socket_name;
    // Server starts detached with a reasonable default window size
    app.attached_clients = 0;

    // Bind the control listener BEFORE loading config so that run-shell
    // commands spawned by load_config can connect back to the server.
    let (tx, rx) = mpsc::channel::<CtrlReq>();
    app.control_rx = Some(rx);
    app.control_tx = Some(tx.clone());
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    app.control_port = Some(port);

    // Write port and key files IMMEDIATELY after binding, BEFORE loading
    // config or creating windows.  run-shell scripts (e.g. PPM) need the
    // port file to discover the server, and the client polls for it to know
    // the server is ready.
    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .unwrap_or_default();
    let dir = format!("{}\\.psmux", home);
    let _ = std::fs::create_dir_all(&dir);

    // Generate a random session key for security
    let session_key: String = {
        use std::collections::hash_map::RandomState;
        use std::hash::{BuildHasher, Hasher};
        let s = RandomState::new();
        let mut h = s.build_hasher();
        h.write_u64(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
        );
        h.write_u64(std::process::id() as u64);
        format!("{:016x}", h.finish())
    };

    app.session_key = session_key.clone();

    let regpath = format!("{}\\{}.port", dir, app.port_file_base());
    let _ = std::fs::write(&regpath, port.to_string());
    let keypath = format!("{}\\{}.key", dir, app.port_file_base());
    let _ = std::fs::write(&keypath, &session_key);
    // Write version stamp so clients can detect stale warm servers (#110).
    let verpath = format!("{}\\{}.version", dir, app.port_file_base());
    let _ = std::fs::write(&verpath, crate::types::build_version_stamp());

    // R4: Write pipe discovery file early so PsmuxAdapter can detect psmux
    // before the pipe listener thread starts.  The pipe listener also writes
    // this file (idempotent update) once it starts accepting connections.
    let pipe_name = crate::backend::pipe::pipe_path(&app.session_name);
    let pipe_file = format!("{}\\{}.pipe", dir, app.port_file_base());
    let _ = std::fs::write(&pipe_file, &pipe_name);

    // Expose the server identity via env var so that child processes spawned
    // by run-shell (from hooks, keybindings, etc.) can find this server when
    // they call `psmux set -g ...` or other CLI commands.
    crate::util::set_env("PSMUX_TARGET_SESSION", app.port_file_base());

    // Try to set file permissions to user-only (Windows)
    #[cfg(windows)]
    {
        // Recreate key file with restricted permissions
        let _ = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&keypath)
            .map(|mut f| std::io::Write::write_all(&mut f, session_key.as_bytes()));
    }

    // Start accept thread BEFORE load_config so that run-shell commands
    // (e.g. PPM plugin manager) spawned during config parsing can connect
    // to the server.  Without this, run-shell scripts fail silently because
    // there is no TCP listener accepting connections yet.
    // Initialize shared aliases empty — will be populated after load_config.
    let shared_aliases: std::sync::Arc<
        std::sync::RwLock<std::collections::HashMap<String, String>>,
    > = std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()));
    let shared_aliases_main = shared_aliases.clone();

    // Initialize mycel event bus (optional — only when built with --features mycel)
    #[cfg(feature = "mycel")]
    {
        let hostname = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "unknown".into());
        crate::mycel::init_mycel_bus(&format!("psmux@{}", hostname));
    }

    #[cfg(feature = "mycel")]
    crate::mycel::publish_pane_event(
        crate::mycel::topics::SESSION_CREATED,
        &serde_json::json!({
            "session_name": app.session_name,
            "client_id": crate::mycel::mycel_client_id().unwrap_or("unknown"),
        }),
    );

    // Clone tx for the backend pipe listener before moving tx into the TCP accept thread.
    let backend_tx = tx.clone();
    let backend_session_name = app.session_name.clone();
    let backend_session_key = app.session_key.clone();

    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            let session_key_clone = session_key.clone();
            let aliases = shared_aliases.clone();
            thread::spawn(move || {
                connection::handle_connection(stream, tx, &session_key_clone, aliases);
            }); // end per-connection thread
        }
    });

    // Start CustomPaneBackend named pipe listener (Windows only).
    // This provides the JSON-RPC endpoint that Claude Code's TeammateTool
    // uses to spawn/capture/kill agent panes.
    if let Err(e) = crate::backend::pipe::start_pipe_listener(
        &backend_session_name,
        backend_tx,
        backend_session_key,
    ) {
        eprintln!("psmux: warning: failed to start backend pipe: {}", e);
    }

    // Load config AFTER the TCP listener is bound, port/key files are written,
    // and the accept thread is running.  This ensures that run-shell commands
    // in the config (e.g. `run '~/.psmux/plugins/ppm/ppm.ps1'`) can connect
    // back to the server to apply settings.

    // Apply initial dimensions BEFORE warm pane spawn so spawn_warm_pane()
    // uses the correct terminal size.
    if let Some((w, h)) = init_size {
        app.last_window_area = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        };
    }

    // Pre-spawn a warm pane BEFORE loading config: the shell (pwsh) starts
    // loading immediately and runs in parallel with config parsing / plugin
    // initialization.  By the time create_window() consumes it, the shell
    // has had the full config-load duration (~100-500ms) as a head start.
    // Only when we have real dimensions and default shell (no custom command).
    let mut early_warm = if init_size.is_some()
        && initial_command.is_none()
        && raw_command.is_none()
        && start_dir.is_none()
    {
        spawn_warm_pane(&*pty_system, &mut app).ok()
    } else {
        None
    };

    load_config(&mut app);

    // If config enabled allow-predictions, the early warm pane was spawned
    // with the wrong PSReadLine init string (predictions disabled). Kill it
    // and respawn with the correct init string that preserves the user's
    // PredictionViewStyle setting (#165).
    if app.allow_predictions {
        if let Some(mut stale) = early_warm.take() {
            stale.child.kill().ok();
        }
    }
    // Refill early_warm if it was killed (or never spawned)
    let early_warm = if early_warm.is_none()
        && init_size.is_some()
        && initial_command.is_none()
        && raw_command.is_none()
        && start_dir.is_none()
    {
        spawn_warm_pane(&*pty_system, &mut app).ok()
    } else {
        early_warm
    };

    // Apply --layout file if PSMUX_LAYOUT_FILE env var is set
    if let Ok(layout_path) = std::env::var("PSMUX_LAYOUT_FILE") {
        std::env::remove_var("PSMUX_LAYOUT_FILE"); // consume it
        if !layout_path.is_empty() {
            match crate::layout::load_layout_file(&layout_path) {
                Ok(layout) => {
                    if let Err(e) = crate::layout::apply_layout_file(&mut app, &*pty_system, layout)
                    {
                        eprintln!("Layout file error: {}", e);
                    }
                    resize_all_panes(&mut app);
                }
                Err(e) => {
                    eprintln!("Failed to load layout file '{}': {}", layout_path, e);
                }
            }
        }
    }

    // Execute queued plugin .ps1 scripts (e.g. theme plugins that use
    // PowerShell variables and call back to psmux via CLI).  We spawn
    // them async and then drain the CtrlReq channel in a mini-loop so
    // show-options / set requests from the scripts are handled before
    // the main UI starts.
    if !app.pending_plugin_scripts.is_empty() {
        let scripts: Vec<String> = app.pending_plugin_scripts.drain(..).collect();
        let target_session = app.port_file_base();
        let mut children: Vec<std::process::Child> = Vec::new();
        for ps1 in &scripts {
            let mut cmd = std::process::Command::new("pwsh");
            cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", ps1]);
            if !target_session.is_empty() {
                cmd.env("PSMUX_TARGET_SESSION", &target_session);
            }
            cmd.stdout(std::process::Stdio::null());
            cmd.stderr(std::process::Stdio::null());
            if let Ok(child) = cmd.spawn() {
                children.push(child);
            }
        }

        // Drain CtrlReq messages until all scripts finish (max 5s).
        if !children.is_empty() {
            let deadline = Instant::now() + Duration::from_secs(5);
            // Temporarily take rx out of app to avoid borrow conflict
            if let Some(rx) = app.control_rx.take() {
                loop {
                    let all_done = children
                        .iter_mut()
                        .all(|c| matches!(c.try_wait(), Ok(Some(_))));
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if all_done || remaining.is_zero() {
                        while let Ok(req) = rx.try_recv() {
                            drain_plugin_req(&mut app, req, &shared_aliases_main);
                        }
                        break;
                    }
                    match rx.recv_timeout(Duration::from_millis(50).min(remaining)) {
                        Ok(req) => drain_plugin_req(&mut app, req, &shared_aliases_main),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(_) => break,
                    }
                }
                app.control_rx = Some(rx);
            }
        }
    }

    // If the user configured a custom default-shell in their config, the
    // early warm pane has the wrong shell — kill it so create_window falls
    // through to a cold spawn with the correct shell.
    // Also kill it if warm was disabled via config (set -g warm off).
    if let Some(wp) = early_warm {
        if !app.warm_enabled {
            // Warm disabled by config — kill the early warm pane
            let mut wp = wp;
            wp.child.kill().ok();
        } else if app.default_shell.is_empty() {
            // No custom shell — the pre-spawned default (pwsh) is correct.
            // Check if config loaded env vars that the early warm pane is missing.
            let needs_env = app.environment.iter().any(|(k, _)| {
                !k.starts_with("PSMUX_TARGET_SESSION") && k != "TMUX" && k != "TMUX_PANE"
            });
            if needs_env {
                // The early warm pane was spawned before config, so it lacks
                // config-defined env vars (e.g. TERM from default-terminal).
                // Kill it and respawn with env vars set at the process level
                // via CommandBuilder::env() — this avoids writing PowerShell
                // commands to the PTY which would echo visibly (#137).
                let mut wp = wp;
                wp.child.kill().ok();
                match spawn_warm_pane(&*pty_system, &mut app) {
                    Ok(new_wp) => {
                        app.warm_pane = Some(new_wp);
                    }
                    Err(e) => {
                        eprintln!("psmux: warm pane respawn failed: {e}");
                    }
                }
            } else {
                app.warm_pane = Some(wp);
            }
        } else {
            // Custom shell set by config — wrong warm pane, kill it
            let mut wp = wp;
            wp.child.kill().ok();
        }
    }

    // Update shared aliases now that config has been loaded
    if let Ok(mut w) = shared_aliases_main.write() {
        *w = app.command_aliases.clone();
    }

    // Create initial window — if a warm pane was pre-spawned above,
    // create_window's fast path transplants it instantly.
    let saved_dir = if start_dir.is_some() {
        env::current_dir().ok()
    } else {
        None
    };
    if let Some(ref dir) = start_dir {
        env::set_current_dir(dir).ok();
    }
    let create_result = if let Some(ref raw_args) = raw_command {
        create_window_raw(&*pty_system, &mut app, raw_args)
    } else {
        create_window(
            &*pty_system,
            &mut app,
            initial_command.as_deref(),
            None,
            None,
        )
    };
    if let Err(e) = create_result {
        // Clean up port/key/version/pipe files so stale entries are not left
        // behind when the pane command fails to spawn (issue #204).
        let _ = std::fs::remove_file(&regpath);
        let _ = std::fs::remove_file(&keypath);
        let _ = std::fs::remove_file(&verpath);
        let _ = std::fs::remove_file(&pipe_file);
        if let Some(mut wp) = app.warm_pane.take() {
            wp.child.kill().ok();
        }
        return Err(e);
    }
    if let Some(prev) = saved_dir {
        env::set_current_dir(prev).ok();
    }
    // Apply window name if specified via -n
    if let Some(n) = window_name {
        if let Some(w) = app.windows.last_mut() {
            w.name = n;
        }
    }
    // Replenish: spawn a warm pane for the NEXT new-window / split.
    // Always replenish when no warm pane is available.
    if app.warm_pane.is_none() {
        match spawn_warm_pane(&*pty_system, &mut app) {
            Ok(wp) => {
                app.warm_pane = Some(wp);
            }
            Err(e) => {
                eprintln!("psmux: warm pane pre-spawn failed: {e}");
            }
        }
    }
    // Fire client-attached hooks once at startup so plugins populate initial
    // data (e.g. CPU/battery) even for detached sessions (tppanel previews).
    {
        crate::commands::fire_hooks(&mut app, "client-attached");
    }
    // Spawn a warm server for the NEXT new-session when the current session
    // is allowed to keep background state alive.
    if should_spawn_warm_server(&app) {
        spawn_warm_server(&app);
    }
    let mut state_dirty = true;
    // Debounce counter for PTY data readiness.  When data arrives, set to 5;
    // decrement each iteration.  Use 1ms timeout only while counter > 0,
    // preventing continuous 1ms polling when a pane produces steady output
    // (e.g., Claude Code running tool calls).
    #[allow(unused_assignments)]
    let mut data_ready_ticks: u8 = 0;
    let mut cached_dump_state = String::new();
    let mut cached_data_version: u64 = 0;
    // Cached metadata JSON — windows/tree/prefix change only on structural
    // mutations, so we rebuild them lazily via `meta_dirty`.
    let mut meta_dirty = true;
    let mut cached_windows_json = String::new();
    let mut cached_tree_json = String::new();
    let mut cached_prefix_str = String::new();
    let mut cached_prefix2_str = String::new();
    let mut cached_base_index: usize = 0;
    let mut cached_pred_dim: bool = false;
    let mut cached_status_style = String::new();
    let mut cached_bindings_json = String::from("[]");
    // Reusable buffer for building the combined JSON envelope.
    let mut combined_buf = String::with_capacity(32768);

    // Track when we recently sent keystrokes to the PTY.  While waiting
    // for the echo to appear we use a much shorter recv_timeout (1ms vs 5ms)
    // so that dump-state requests are served with minimal delay.  This is
    // critical for nested-shell latency (e.g. WSL inside pwsh) where the
    // echo path goes through ConPTY → pwsh → WSL → echo → ConPTY and can
    // take 10-30ms.  Without this, each "no-change" polling cycle costs up
    // to 5ms, adding cumulative latency visible as heavy input lag.
    let mut echo_pending_until: Option<Instant> = None;

    // Track when any client last requested a dump or sent input.
    // Used to ramp down the server loop frequency when truly idle.
    let mut last_client_activity = Instant::now();

    // Throttle reap_children: only check for exited processes every 250ms.
    // With hundreds of windows, calling try_wait() on every process each
    // loop iteration wastes CPU.  Exited processes are still reaped promptly
    // (250ms is imperceptible to users).
    let mut last_reap = Instant::now();

    // Persist temp_focus_restore across batch boundaries so that a
    // FocusWindowTemp/FocusPaneByIndexTemp in one batch plus the actual
    // command (e.g. CapturePane) in the next batch still works correctly.
    let mut temp_focus_restore: Option<(usize, usize)> = None;

    loop {
        // Adaptive timeout: ramps from 1ms (active typing/echo) through
        // 5ms (client recently active) up to 50ms (fully idle).  This
        // dramatically reduces CPU usage when the session is idle while
        // keeping responsiveness high during interaction.
        let data_ready =
            crate::types::PTY_DATA_READY.swap(false, std::sync::atomic::Ordering::AcqRel);
        if data_ready {
            state_dirty = true;
            // Reset debounce: stay responsive for ~5 iterations (5ms) after
            // each burst of PTY output, then ramp timeout back up.  This
            // prevents 1ms polling when a pane produces continuous output
            // (e.g., Claude Code tool calls), reducing CPU from ~30% to ~5%.
            data_ready_ticks = 5;
        } else {
            data_ready_ticks = data_ready_ticks.saturating_sub(1);
        }
        // When a popup PTY is active, always push frames so interactive
        // content (e.g. fzf, shell prompts) updates in real-time.
        if matches!(app.mode, Mode::PopupMode { .. }) {
            state_dirty = true;
        }
        let echo_active = echo_pending_until.is_some_and(|t| t.elapsed().as_millis() < 50);
        let idle_secs = last_client_activity.elapsed().as_secs();
        let timeout_ms: u64 = if echo_active || data_ready_ticks > 0 {
            1 // Active echo/data: 1ms for maximum responsiveness (debounced)
        } else if idle_secs < 2 {
            5 // Recently active: 5ms (200 Hz)
        } else if crate::types::has_frame_receivers() {
            16 // Push clients attached: 16ms (~60 Hz) so PTY data
               // is detected and pushed within one vsync period.
        } else {
            50 // No clients: 50ms (20 Hz) — saves CPU
        };
        if let Some(rx) = app.control_rx.as_ref() {
            if let Ok(req) = rx.recv_timeout(Duration::from_millis(timeout_ms)) {
                last_client_activity = Instant::now();
                let mut pending = vec![req];
                // Drain any additional queued messages without blocking
                while let Ok(r) = rx.try_recv() {
                    pending.push(r);
                }
                // Also check if fresh PTY output arrived while we were
                // waiting – mark state dirty so DumpState produces a full
                // frame instead of "NC".
                if crate::types::PTY_DATA_READY.swap(false, std::sync::atomic::Ordering::AcqRel) {
                    state_dirty = true;
                }
                // Process key/command inputs BEFORE dump-state requests.
                // This ensures ConPTY receives keystrokes before we serialize
                // the screen, reducing stale-frame responses.
                pending.sort_by_key(|r| match r {
                    CtrlReq::DumpState(..) => 1,
                    CtrlReq::DumpLayout(_) => 1,
                    _ => 0,
                });
                // Track temporary -t focus: save (active_idx, pane_id) when
                // FocusWindowTemp/FocusPaneTemp is seen, restore after next
                // non-temp command so the user's view doesn't jump.
                // We store the pane ID (not path) because kill-pane
                // restructures the tree, invalidating saved paths (#71).
                // NOTE: temp_focus_restore lives outside the loop so it
                // persists across batch boundaries (prevents race where
                // FocusWindowTemp and the actual command land in different
                // batches).
                for req in pending {
                    let mutates_state = !matches!(
                        &req,
                        CtrlReq::DumpState(..)
                            | CtrlReq::SendText(_)
                            | CtrlReq::SendKey(_)
                            | CtrlReq::SendPaste(_)
                            | CtrlReq::SendKeysHex(_)
                    );
                    let is_temp_focus = matches!(
                        &req,
                        CtrlReq::FocusWindowTemp(_)
                            | CtrlReq::FocusPaneTemp(_)
                            | CtrlReq::FocusPaneTempCheck(..)
                            | CtrlReq::FocusPaneByIndexTemp(_)
                    );
                    let mut hook_event: Option<&str> = None;
                    // Track active_idx changes for debugging window-switch issues
                    let _prev_active_idx = app.active_idx;
                    let _req_tag: &str = match &req {
                        CtrlReq::NextWindow => "NextWindow",
                        CtrlReq::PrevWindow => "PrevWindow",
                        CtrlReq::SelectWindow(_) => "SelectWindow",
                        CtrlReq::FocusWindow(_) => "FocusWindow",
                        CtrlReq::FocusWindowTemp(_) => "FocusWindowTemp",
                        CtrlReq::FocusWindowCmd(_) => "FocusWindowCmd",
                        CtrlReq::LastWindow => "LastWindow",
                        CtrlReq::MouseDown(..) => "MouseDown",
                        CtrlReq::MouseDownRight(..) => "MouseDownRight",
                        CtrlReq::MouseDownMiddle(..) => "MouseDownMiddle",
                        CtrlReq::FocusPane(_) => "FocusPane",
                        CtrlReq::FocusPaneTemp(_) => "FocusPaneTemp",
                        CtrlReq::FocusPaneTempCheck(..) => "FocusPaneTempCheck",
                        CtrlReq::NewWindow(..) => "NewWindow",
                        CtrlReq::NewWindowRaw(..) => "NewWindowRaw",
                        CtrlReq::KillWindow => "KillWindow",
                        CtrlReq::KillPane => "KillPane",
                        CtrlReq::KillPaneById(_) => "KillPaneById",
                        CtrlReq::BreakPane => "BreakPane",
                        CtrlReq::JoinPane(_) => "JoinPane",
                        CtrlReq::MoveWindow(..) => "MoveWindow",
                        CtrlReq::SwapWindow(_) => "SwapWindow",
                        CtrlReq::Exec { .. } => "Exec",
                        _ => "",
                    };
                    match req {
                        CtrlReq::NewWindow(cmd, name, detached, start_dir, env_vars, shell) => {
                            let prev_idx = app.active_idx;
                            // Expand format variables like #{pane_current_path} (#111)
                            let start_dir = start_dir
                                .map(|d| expand_format(&d, &app))
                                .filter(|d: &String| !d.is_empty());
                            let saved_dir = if start_dir.is_some() {
                                env::current_dir().ok()
                            } else {
                                None
                            };
                            if let Some(dir) = &start_dir {
                                env::set_current_dir(dir).ok();
                            }
                            // Hide the warm pane when an explicit start dir, env vars,
                            // or shell override is requested — the warm pane uses the default shell.
                            let stashed_warm =
                                if start_dir.is_some() || !env_vars.is_empty() || shell.is_some() {
                                    app.warm_pane.take()
                                } else {
                                    None
                                };
                            // Inject -e env vars at both levels:
                            // 1) app.environment → apply_user_environment adds to CommandBuilder
                            // 2) process env → get_base_env() captures during CommandBuilder::new()
                            for (k, v) in &env_vars {
                                app.environment.insert(k.clone(), v.clone());
                                crate::util::set_env(k, v);
                            }
                            if let Err(e) = create_window(
                                &*pty_system,
                                &mut app,
                                cmd.as_deref(),
                                start_dir.as_deref(),
                                shell.as_deref(),
                            ) {
                                eprintln!("psmux: new-window error: {e}");
                            }
                            // Remove temporary env vars from both levels
                            for (k, _) in &env_vars {
                                app.environment.remove(k);
                                crate::util::remove_env(k);
                            }
                            if let Some(wp) = stashed_warm {
                                app.warm_pane = Some(wp);
                            }
                            if let Some(prev) = saved_dir {
                                env::set_current_dir(prev).ok();
                            }
                            if let Some(n) = name {
                                if let Some(w) = app.windows.last_mut() {
                                    w.name = n;
                                    w.manual_rename = true;
                                }
                            }
                            if detached {
                                app.active_idx = prev_idx;
                            }
                            // Replenish warm pane pool for next new-window
                            if app.warm_pane.is_none() {
                                if let Ok(wp) = spawn_warm_pane(&*pty_system, &mut app) {
                                    app.warm_pane = Some(wp);
                                }
                            }
                            resize_all_panes(&mut app);
                            meta_dirty = true;
                            hook_event = Some("after-new-window");
                            crate::resurrection::save_snapshot(&app);
                        }
                        CtrlReq::NewWindowPrint(
                            cmd,
                            name,
                            detached,
                            start_dir,
                            format_str,
                            env_vars,
                            shell,
                            resp,
                        ) => {
                            let prev_idx = app.active_idx;
                            let start_dir = start_dir
                                .map(|d| expand_format(&d, &app))
                                .filter(|d: &String| !d.is_empty());
                            let saved_dir = if start_dir.is_some() {
                                env::current_dir().ok()
                            } else {
                                None
                            };
                            if let Some(dir) = &start_dir {
                                env::set_current_dir(dir).ok();
                            }
                            let stashed_warm =
                                if start_dir.is_some() || !env_vars.is_empty() || shell.is_some() {
                                    app.warm_pane.take()
                                } else {
                                    None
                                };
                            // Inject -e env vars at both levels:
                            // 1) app.environment → apply_user_environment adds to CommandBuilder
                            // 2) process env → get_base_env() captures during CommandBuilder::new()
                            for (k, v) in &env_vars {
                                app.environment.insert(k.clone(), v.clone());
                                crate::util::set_env(k, v);
                            }
                            if let Err(e) = create_window(
                                &*pty_system,
                                &mut app,
                                cmd.as_deref(),
                                start_dir.as_deref(),
                                shell.as_deref(),
                            ) {
                                eprintln!("psmux: new-window error: {e}");
                            }
                            // Remove temporary env vars from both levels
                            for (k, _) in &env_vars {
                                app.environment.remove(k);
                                crate::util::remove_env(k);
                            }
                            if let Some(wp) = stashed_warm {
                                app.warm_pane = Some(wp);
                            }
                            if let Some(prev) = saved_dir {
                                env::set_current_dir(prev).ok();
                            }
                            if let Some(n) = name {
                                if let Some(w) = app.windows.last_mut() {
                                    w.name = n;
                                    w.manual_rename = true;
                                }
                            }
                            // Use full format engine for -P output (tmux compatible)
                            let new_win_idx = app.windows.len() - 1;
                            let fmt = format_str
                                .as_deref()
                                .unwrap_or("#{session_name}:#{window_index}");
                            let pane_info =
                                crate::format::expand_format_for_window(fmt, &app, new_win_idx);
                            if detached {
                                app.active_idx = prev_idx;
                            }
                            let _ = resp.send(pane_info);
                            // Replenish warm pane pool for next new-window
                            if app.warm_pane.is_none() {
                                if let Ok(wp) = spawn_warm_pane(&*pty_system, &mut app) {
                                    app.warm_pane = Some(wp);
                                }
                            }
                            resize_all_panes(&mut app);
                            meta_dirty = true;
                            hook_event = Some("after-new-window");
                        }
                        CtrlReq::NewWindowRaw(argv, name, detached, start_dir) => {
                            let prev_idx = app.active_idx;
                            let start_dir = start_dir
                                .map(|d| expand_format(&d, &app))
                                .filter(|d: &String| !d.is_empty());
                            let saved_dir = if start_dir.is_some() {
                                env::current_dir().ok()
                            } else {
                                None
                            };
                            if let Some(dir) = &start_dir {
                                env::set_current_dir(dir).ok();
                            }
                            let stashed_warm = app.warm_pane.take();
                            if let Err(e) = create_window_raw(&*pty_system, &mut app, &argv) {
                                eprintln!("psmux: new-window-raw error: {e}");
                            }
                            app.warm_pane = stashed_warm;
                            if let Some(prev) = saved_dir {
                                env::set_current_dir(prev).ok();
                            }
                            if let Some(n) = name {
                                if let Some(w) = app.windows.last_mut() {
                                    w.name = n;
                                    w.manual_rename = true;
                                }
                            }
                            if detached {
                                app.active_idx = prev_idx;
                            }
                            resize_all_panes(&mut app);
                            meta_dirty = true;
                            hook_event = Some("after-new-window");
                        }
                        CtrlReq::NewWindowRawPrint(argv, name, detached, start_dir, format_str, resp) => {
                            let prev_idx = app.active_idx;
                            let start_dir = start_dir
                                .map(|d| expand_format(&d, &app))
                                .filter(|d: &String| !d.is_empty());
                            let saved_dir = if start_dir.is_some() {
                                env::current_dir().ok()
                            } else {
                                None
                            };
                            if let Some(dir) = &start_dir {
                                env::set_current_dir(dir).ok();
                            }
                            let stashed_warm = app.warm_pane.take();
                            if let Err(e) = create_window_raw(&*pty_system, &mut app, &argv) {
                                eprintln!("psmux: new-window-raw error: {e}");
                            }
                            app.warm_pane = stashed_warm;
                            if let Some(prev) = saved_dir {
                                env::set_current_dir(prev).ok();
                            }
                            if let Some(n) = name {
                                if let Some(w) = app.windows.last_mut() {
                                    w.name = n;
                                    w.manual_rename = true;
                                }
                            }
                            let new_win_idx = app.windows.len() - 1;
                            let fmt = format_str.as_deref().unwrap_or("#{session_name}:#{window_index}");
                            let pane_info = crate::format::expand_format_for_window(fmt, &app, new_win_idx);
                            if detached {
                                app.active_idx = prev_idx;
                            }
                            let _ = resp.send(pane_info);
                            resize_all_panes(&mut app);
                            meta_dirty = true;
                            hook_event = Some("after-new-window");
                        }
                        CtrlReq::SplitWindow(
                            k,
                            cmd,
                            detached,
                            start_dir,
                            size_pct,
                            env_vars,
                            shell,
                            resp,
                        ) => {
                            // tmux: split-window without -Z permanently unzooms (#82)
                            unzoom_if_zoomed(&mut app);
                            let start_dir = start_dir
                                .map(|d| expand_format(&d, &app))
                                .filter(|d: &String| !d.is_empty());
                            let saved_dir = if start_dir.is_some() {
                                env::current_dir().ok()
                            } else {
                                None
                            };
                            if let Some(dir) = &start_dir {
                                env::set_current_dir(dir).ok();
                            }
                            let prev_path = app.windows[app.active_idx].active_path.clone();
                            // Hide warm pane when explicit start_dir, env vars, or shell override given
                            let stashed_warm =
                                if start_dir.is_some() || !env_vars.is_empty() || shell.is_some() {
                                    app.warm_pane.take()
                                } else {
                                    None
                                };
                            // Temporarily inject -e env vars so apply_user_environment picks them up
                            for (k_env, v_env) in &env_vars {
                                app.environment.insert(k_env.clone(), v_env.clone());
                            }
                            if let Err(e) = split_active_with_command(
                                &mut app,
                                k,
                                cmd.as_deref(),
                                Some(&*pty_system),
                                start_dir.as_deref(),
                                shell.as_deref(),
                            ) {
                                let _ = resp.send(format!("psmux: split-window: {e}"));
                            } else {
                                let _ = resp.send(String::new());
                            }
                            // Remove temporary env vars from both levels
                            for (k_env, _) in &env_vars {
                                app.environment.remove(k_env);
                                crate::util::remove_env(k_env);
                            }
                            if let Some(wp) = stashed_warm {
                                app.warm_pane = Some(wp);
                            }
                            // Apply size if specified (as percentage)
                            if let Some(pct) = size_pct {
                                let pct = pct.clamp(1, 99);
                                let win = &mut app.windows[app.active_idx];
                                if let Some(Node::Split { sizes, .. }) =
                                    get_split_mut(&mut win.root, &prev_path)
                                {
                                    sizes[0] = 100 - pct;
                                    sizes[1] = pct;
                                }
                            }
                            if detached {
                                // Capture new pane ID before reverting focus
                                let new_pane_id = crate::tree::get_active_pane_id(
                                    &app.windows[app.active_idx].root,
                                    &app.windows[app.active_idx].active_path,
                                );
                                // Revert focus to the previously active pane.
                                // After split, prev_path now points to a Split node;
                                // the original pane is child [0] of that Split.
                                let mut revert_path = prev_path;
                                revert_path.push(0);
                                app.windows[app.active_idx].active_path = revert_path;
                                // Detached splits never focus the new pane — remove
                                // from MRU entirely so directional nav tie-breaks by
                                // pane_index among equally-unvisited candidates (#70).
                                if let Some(nid) = new_pane_id {
                                    let win = &mut app.windows[app.active_idx];
                                    win.pane_mru.retain(|&id| id != nid);
                                }
                            } else {
                                // Non-detached: new pane keeps focus.
                                // Cancel temp_focus_restore so -t doesn't revert (#112).
                                temp_focus_restore = None;
                                // Explicitly focus the new pane to ensure it's active (#112)
                                let new_pane_id = crate::tree::get_active_pane_id(
                                    &app.windows[app.active_idx].root,
                                    &app.windows[app.active_idx].active_path,
                                );
                                if let Some(pid) = new_pane_id {
                                    focus_pane_by_id(&mut app, pid);
                                }
                            }
                            if let Some(prev) = saved_dir {
                                env::set_current_dir(prev).ok();
                            }
                            // Replenish warm pane for the next new-window/split
                            if app.warm_pane.is_none() {
                                if let Ok(wp) = spawn_warm_pane(&*pty_system, &mut app) {
                                    app.warm_pane = Some(wp);
                                }
                            }
                            resize_all_panes(&mut app);
                            meta_dirty = true;
                            hook_event = Some("after-split-window");
                            crate::resurrection::save_snapshot(&app);
                        }
                        CtrlReq::SplitWindowPrint(
                            k,
                            cmd,
                            detached,
                            start_dir,
                            size_pct,
                            format_str,
                            env_vars,
                            shell,
                            resp,
                        ) => {
                            unzoom_if_zoomed(&mut app);
                            let start_dir = start_dir
                                .map(|d| expand_format(&d, &app))
                                .filter(|d: &String| !d.is_empty());
                            let saved_dir = if start_dir.is_some() {
                                env::current_dir().ok()
                            } else {
                                None
                            };
                            if let Some(dir) = &start_dir {
                                env::set_current_dir(dir).ok();
                            }
                            let prev_path = app.windows[app.active_idx].active_path.clone();
                            let stashed_warm =
                                if start_dir.is_some() || !env_vars.is_empty() || shell.is_some() {
                                    app.warm_pane.take()
                                } else {
                                    None
                                };
                            // Temporarily inject -e env vars so apply_user_environment picks them up
                            for (k_env, v_env) in &env_vars {
                                app.environment.insert(k_env.clone(), v_env.clone());
                            }
                            let split_err = match split_active_with_command(
                                &mut app,
                                k,
                                cmd.as_deref(),
                                Some(&*pty_system),
                                start_dir.as_deref(),
                                shell.as_deref(),
                            ) {
                                Ok(()) => None,
                                Err(e) => {
                                    eprintln!("psmux: split-window error: {e}");
                                    Some(e.to_string())
                                }
                            };
                            let split_ok = split_err.is_none();
                            // Remove temporary env vars from both levels
                            for (k_env, _) in &env_vars {
                                app.environment.remove(k_env);
                                crate::util::remove_env(k_env);
                            }
                            if let Some(wp) = stashed_warm {
                                app.warm_pane = Some(wp);
                            }
                            if split_ok {
                                if let Some(pct) = size_pct {
                                    let pct = pct.clamp(1, 99);
                                    let win = &mut app.windows[app.active_idx];
                                    if let Some(Node::Split { sizes, .. }) =
                                        get_split_mut(&mut win.root, &prev_path)
                                    {
                                        sizes[0] = 100 - pct;
                                        sizes[1] = pct;
                                    }
                                }
                                // Use full format engine for -P output (tmux compatible)
                                let fmt = format_str
                                    .as_deref()
                                    .unwrap_or("#{session_name}:#{window_index}.#{pane_index}");
                                let pane_info = crate::format::expand_format_for_window(
                                    fmt,
                                    &app,
                                    app.active_idx,
                                );
                                if detached {
                                    // Capture new pane ID before reverting focus
                                    let new_pane_id = crate::tree::get_active_pane_id(
                                        &app.windows[app.active_idx].root,
                                        &app.windows[app.active_idx].active_path,
                                    );
                                    let mut revert_path = prev_path;
                                    revert_path.push(0);
                                    app.windows[app.active_idx].active_path = revert_path;
                                    // Detached splits: remove from MRU (#70 pane_index tie-break)
                                    if let Some(nid) = new_pane_id {
                                        let win = &mut app.windows[app.active_idx];
                                        win.pane_mru.retain(|&id| id != nid);
                                    }
                                } else {
                                    temp_focus_restore = None;
                                    // Explicitly focus the new pane to ensure it's active (#112)
                                    let new_pane_id = crate::tree::get_active_pane_id(
                                        &app.windows[app.active_idx].root,
                                        &app.windows[app.active_idx].active_path,
                                    );
                                    if let Some(pid) = new_pane_id {
                                        focus_pane_by_id(&mut app, pid);
                                    }
                                }
                                let _ = resp.send(pane_info);
                            } else {
                                // Signal error to the client with a prefix it can detect
                                let err_msg =
                                    split_err.as_deref().unwrap_or("pane too small to split");
                                let _ = resp.send(format!("ERROR:{err_msg}"));
                            }
                            if let Some(prev) = saved_dir {
                                env::set_current_dir(prev).ok();
                            }
                            // Replenish warm pane
                            if app.warm_pane.is_none() {
                                if let Ok(wp) = spawn_warm_pane(&*pty_system, &mut app) {
                                    app.warm_pane = Some(wp);
                                }
                            }
                            if split_ok {
                                resize_all_panes(&mut app);
                                meta_dirty = true;
                                hook_event = Some("after-split-window");
                            }
                        }
                        CtrlReq::KillPane => {
                            unzoom_if_zoomed(&mut app);
                            let target_win_idx = app.active_idx;
                            let _ = kill_active_pane(&mut app);
                            // If the window now has only a single dead pane (root Leaf
                            // with killed process), remove the window immediately instead
                            // of waiting for the reaper tick.  This prevents the brief
                            // "respawn" artifact where the dead pane lingers visibly.
                            if target_win_idx < app.windows.len()
                                && crate::tree::count_panes(&app.windows[target_win_idx].root) <= 1
                            {
                                if let Node::Leaf(ref mut p) = app.windows[target_win_idx].root {
                                    if (p.child.try_wait().ok().flatten().is_some()
                                        || p.dead
                                        || p.killed)
                                        && app.windows.len() > 1
                                    {
                                        app.windows.remove(target_win_idx);
                                        if app.active_idx >= app.windows.len() {
                                            app.active_idx = app.windows.len().saturating_sub(1);
                                        }
                                    }
                                }
                            }
                            resize_all_panes(&mut app);
                            meta_dirty = true;
                            hook_event = Some("after-kill-pane");
                            crate::resurrection::save_snapshot(&app);
                        }
                        CtrlReq::KillPaneById(pid) => {
                            unzoom_if_zoomed(&mut app);
                            // Find which window contains the pane before killing it
                            let target_win_idx = app
                                .windows
                                .iter()
                                .position(|w| crate::tree::find_path_by_id(&w.root, pid).is_some());
                            let _ = kill_pane_by_id(&mut app, pid);
                            // Immediately remove the window if its last pane was killed
                            if let Some(wi) = target_win_idx {
                                if wi < app.windows.len()
                                    && crate::tree::count_panes(&app.windows[wi].root) <= 1
                                {
                                    if let Node::Leaf(ref mut p) = app.windows[wi].root {
                                        if (p.child.try_wait().ok().flatten().is_some()
                                            || p.dead
                                            || p.killed)
                                            && app.windows.len() > 1
                                        {
                                            app.windows.remove(wi);
                                            if app.active_idx >= app.windows.len() {
                                                app.active_idx =
                                                    app.windows.len().saturating_sub(1);
                                            }
                                        }
                                    }
                                }
                            }
                            resize_all_panes(&mut app);
                            meta_dirty = true;
                            hook_event = Some("after-kill-pane");
                        }
                        CtrlReq::CapturePane(resp) => {
                            if let Some(text) = capture_active_pane_text(&mut app)? {
                                let _ = resp.send(text);
                            } else {
                                let _ = resp.send(String::new());
                            }
                        }
                        CtrlReq::CapturePaneStyled(resp, s, e) => {
                            if let Some(text) = capture_active_pane_styled(&mut app, s, e)? {
                                let _ = resp.send(text);
                            } else {
                                let _ = resp.send(String::new());
                            }
                        }
                        CtrlReq::CapturePaneRange(resp, s, e) => {
                            if let Some(text) = capture_active_pane_range(&mut app, s, e)? {
                                let _ = resp.send(text);
                            } else {
                                let _ = resp.send(String::new());
                            }
                        }
                        CtrlReq::CapturePaneClean(resp) => {
                            if let Some(text) = capture_active_pane_text_clean(&mut app)? {
                                let _ = resp.send(text);
                            } else {
                                let _ = resp.send(String::new());
                            }
                        }
                        CtrlReq::GetPaneContents { pane_id, resp } => {
                            let text = read_pane_contents(&app, pane_id);
                            let _ = resp.send(text);
                        }
                        CtrlReq::FocusWindow(wid) => {
                            // wid is a display index (same as tmux window number), convert to internal array index
                            if wid >= app.window_base_index {
                                let internal_idx = wid - app.window_base_index;
                                if internal_idx < app.windows.len()
                                    && internal_idx != app.active_idx
                                {
                                    switch_with_copy_save(&mut app, |app| {
                                        app.last_window_idx = app.active_idx;
                                        app.active_idx = internal_idx;
                                    });
                                    // Clear activity/bell/silence flags on the newly-focused window
                                    if let Some(win) = app.windows.get_mut(internal_idx) {
                                        win.activity_flag = false;
                                        win.bell_flag = false;
                                        win.silence_flag = false;
                                    }
                                    // Lazily resize panes in the newly-focused window
                                    resize_all_panes(&mut app);
                                }
                            }
                            meta_dirty = true;
                        }
                        CtrlReq::FocusPane(pid) => {
                            let old_path = app.windows[app.active_idx].active_path.clone();
                            switch_with_copy_save(&mut app, |app| {
                                focus_pane_by_id(app, pid);
                            });
                            let new_path = app.windows[app.active_idx].active_path.clone();
                            if new_path != old_path {
                                helpers::send_focus_events(&mut app, &old_path, &new_path);
                                unzoom_if_zoomed(&mut app);
                            }
                            meta_dirty = true;
                        }
                        CtrlReq::FocusPaneByIndex(idx) => {
                            let old_path = app.windows[app.active_idx].active_path.clone();
                            switch_with_copy_save(&mut app, |app| {
                                focus_pane_by_index(app, idx);
                            });
                            let new_path = app.windows[app.active_idx].active_path.clone();
                            if new_path != old_path {
                                helpers::send_focus_events(&mut app, &old_path, &new_path);
                                unzoom_if_zoomed(&mut app);
                            }
                            // Update MRU so directional navigation remembers this focus change
                            let win = &mut app.windows[app.active_idx];
                            if let Some(pid) =
                                crate::tree::get_active_pane_id(&win.root, &win.active_path)
                            {
                                crate::tree::touch_mru(&mut win.pane_mru, pid);
                            }
                            meta_dirty = true;
                        }
                        // ── Temporary focus variants for -t targeting ────────────
                        // These switch active_idx/active_path so the NEXT command
                        // in the batch operates on the correct window/pane.
                        // After the entire pending batch is processed, we restore
                        // the original focus (see temp_focus_restore below).
                        CtrlReq::FocusWindowTemp(wid) => {
                            if temp_focus_restore.is_none() {
                                let pane_id = crate::tree::get_active_pane_id(
                                    &app.windows[app.active_idx].root,
                                    &app.windows[app.active_idx].active_path,
                                )
                                .unwrap_or(usize::MAX);
                                temp_focus_restore = Some((app.active_idx, pane_id));
                            }
                            if wid >= app.window_base_index {
                                let internal_idx = wid - app.window_base_index;
                                if internal_idx < app.windows.len() {
                                    app.active_idx = internal_idx;
                                }
                            }
                        }
                        CtrlReq::FocusPaneTemp(pid) => {
                            if temp_focus_restore.is_none() {
                                let pane_id = crate::tree::get_active_pane_id(
                                    &app.windows[app.active_idx].root,
                                    &app.windows[app.active_idx].active_path,
                                )
                                .unwrap_or(usize::MAX);
                                temp_focus_restore = Some((app.active_idx, pane_id));
                            }
                            // Use no_mru variant: temp focus is internal
                            // targeting, not a user navigation action.
                            // Touching MRU here pollutes kill-pane's MRU
                            // fallback (#71, #140).
                            crate::tree::focus_pane_by_id_no_mru(&mut app, pid);
                        }
                        CtrlReq::FocusPaneTempCheck(pid, resp) => {
                            if temp_focus_restore.is_none() {
                                let pane_id = crate::tree::get_active_pane_id(
                                    &app.windows[app.active_idx].root,
                                    &app.windows[app.active_idx].active_path,
                                )
                                .unwrap_or(usize::MAX);
                                temp_focus_restore = Some((app.active_idx, pane_id));
                            }
                            let prev_idx = app.active_idx;
                            let prev_path = app.windows[app.active_idx].active_path.clone();
                            crate::tree::focus_pane_by_id_no_mru(&mut app, pid);
                            // Check if focus actually changed — if not, pane wasn't found
                            let found = app.active_idx != prev_idx
                                || app.windows[app.active_idx].active_path != prev_path
                                || crate::tree::get_active_pane_id(
                                    &app.windows[app.active_idx].root,
                                    &app.windows[app.active_idx].active_path,
                                ) == Some(pid);
                            let _ = resp.send(found);
                        }
                        CtrlReq::FocusPaneByIndexTemp(idx) => {
                            if temp_focus_restore.is_none() {
                                let pane_id = crate::tree::get_active_pane_id(
                                    &app.windows[app.active_idx].root,
                                    &app.windows[app.active_idx].active_path,
                                )
                                .unwrap_or(usize::MAX);
                                temp_focus_restore = Some((app.active_idx, pane_id));
                            }
                            focus_pane_by_index(&mut app, idx);
                        }
                        CtrlReq::SessionInfo(resp) => {
                            let attached = if app.attached_clients > 0 {
                                " (attached)"
                            } else {
                                ""
                            };
                            let windows = app.windows.len();
                            let created = app.created_at.format("%a %b %e %H:%M:%S %Y");
                            let line = format!(
                                "{}: {} windows (created {}){}\n",
                                app.session_name, windows, created, attached
                            );
                            let _ = resp.send(line);
                        }
                        CtrlReq::ClientAttach(cid) => {
                            app.attached_clients = app.attached_clients.saturating_add(1);
                            app.latest_client_id = Some(cid);
                            hook_event = Some("client-attached");
                            // update-environment: refresh env vars from the attaching client's environment
                            let update_vars = app.update_environment.clone();
                            for var_spec in &update_vars {
                                let remove = var_spec.starts_with('-');
                                let name = if remove {
                                    &var_spec[1..]
                                } else {
                                    var_spec.as_str()
                                };
                                if remove {
                                    app.environment.remove(name);
                                } else if let Ok(val) = std::env::var(name) {
                                    app.environment.insert(name.to_string(), val);
                                } else {
                                    app.environment.remove(name);
                                }
                            }
                        }
                        CtrlReq::ClientDetach(cid) => {
                            app.attached_clients = app.attached_clients.saturating_sub(1);
                            app.client_sizes.remove(&cid);
                            app.client_prefix_active = false;
                            if app.latest_client_id == Some(cid) {
                                app.latest_client_id = None;
                            }
                            // Recompute effective size from remaining clients
                            if let Some((w, h)) = compute_effective_client_size(&app) {
                                app.last_window_area = Rect {
                                    x: 0,
                                    y: 0,
                                    width: w,
                                    height: h,
                                };
                                resize_all_panes(&mut app);
                            }
                            hook_event = Some("client-detached");
                            if app.attached_clients == 0 && app.destroy_unattached {
                                let home = env::var("USERPROFILE")
                                    .or_else(|_| env::var("HOME"))
                                    .unwrap_or_default();
                                let regpath =
                                    format!("{}\\.psmux\\{}.port", home, app.port_file_base());
                                let keypath =
                                    format!("{}\\.psmux\\{}.key", home, app.port_file_base());
                                let _ = std::fs::remove_file(&regpath);
                                let _ = std::fs::remove_file(&keypath);
                                crate::types::shutdown_persistent_streams();
                                tree::kill_all_children_batch(&mut app.windows);
                                if let Some(mut wp) = app.warm_pane.take() {
                                    wp.child.kill().ok();
                                }
                                std::thread::sleep(std::time::Duration::from_millis(10));
                                std::process::exit(0);
                            }
                        }
                        CtrlReq::DumpLayout(resp) => {
                            let json = dump_layout_json(&mut app)?;
                            let _ = resp.send(json);
                        }
                        CtrlReq::DumpState(resp, allow_nc) => {
                            // ── Activity / bell / silence detection ──
                            helpers::check_window_activity(&mut app);

                            // ── Propagate OSC 0/2 titles to pane.title ──
                            if helpers::propagate_osc_titles(&mut app) {
                                state_dirty = true;
                            }

                            // ── Automatic rename / allow-rename: resolve window names ──
                            {
                                let in_copy =
                                    matches!(app.mode, Mode::CopyMode | Mode::CopySearch { .. });
                                let auto_rename = app.automatic_rename;
                                let allow_rename = app.allow_rename;
                                if (auto_rename || allow_rename) && !in_copy {
                                    for win in app.windows.iter_mut() {
                                        if win.manual_rename {
                                            continue;
                                        }
                                        if let Some(p) = crate::tree::active_pane_mut(
                                            &mut win.root,
                                            &win.active_path,
                                        ) {
                                            if p.dead {
                                                continue;
                                            }
                                            if p.last_title_check.elapsed().as_millis() < 1000 {
                                                continue;
                                            }
                                            p.last_title_check = std::time::Instant::now();
                                            if p.child_pid.is_none() {
                                                p.child_pid =
                                                    crate::platform::mouse_inject::get_child_pid(
                                                        &*p.child,
                                                    );
                                            }
                                            let new_name = if auto_rename {
                                                // automatic-rename: use foreground process name
                                                if let Some(pid) = p.child_pid {
                                                    match crate::platform::process_info::get_foreground_process_name(pid) {
                                                        Some(name) => name,
                                                        None => {
                                                            // No foreground child found yet.  Keep the
                                                            // current window name rather than flashing
                                                            // to the shell name before a child process
                                                            // spawns (#229).
                                                            continue;
                                                        }
                                                    }
                                                } else if allow_rename && !p.title.is_empty() {
                                                    p.title.clone()
                                                } else {
                                                    continue;
                                                }
                                            } else if allow_rename {
                                                // allow-rename only: use OSC title from child
                                                if let Ok(parser) = p.term.lock() {
                                                    let title = parser.screen().title();
                                                    if !title.is_empty() {
                                                        title.to_string()
                                                    } else {
                                                        continue;
                                                    }
                                                } else {
                                                    continue;
                                                }
                                            } else {
                                                continue;
                                            };
                                            if !new_name.is_empty() && win.name != new_name {
                                                win.name = new_name;
                                                meta_dirty = true;
                                                state_dirty = true;
                                            }
                                        }
                                    }
                                }
                            }
                            // Fast-path: nothing changed at all → 2-byte "NC" marker
                            // instead of cloning 50-100KB of JSON.
                            // Only allowed for persistent connections that already have
                            // the previous frame; one-shot connections always need full state.
                            // Skip the fast-path while any pane in the active window still
                            // has a default placeholder title — we need layout serialisation
                            // to keep running infer_title_from_prompt until a real title is
                            // resolved.
                            let has_placeholder_title = app
                                .windows
                                .get(app.active_idx)
                                .and_then(|w| crate::tree::active_pane(&w.root, &w.active_path))
                                .is_some_and(|p| p.title.starts_with("pane %"));
                            let has_squelch = app
                                .windows
                                .get(app.active_idx)
                                .and_then(|w| crate::tree::active_pane(&w.root, &w.active_path))
                                .is_some_and(|p| p.squelch_until.is_some());
                            if allow_nc
                                && !state_dirty
                                && !has_placeholder_title
                                && !has_squelch
                                && !cached_dump_state.is_empty()
                                && cached_data_version == combined_data_version(&app)
                            {
                                let _ = resp.send("NC".to_string());
                                continue;
                            }
                            // Rebuild metadata cache if structural changes happened.
                            if meta_dirty {
                                cached_windows_json = list_windows_json_with_tabs(&app)?;
                                cached_tree_json = list_tree_json(&app)?;
                                cached_prefix_str = format_key_binding(&app.prefix_key);
                                cached_prefix2_str = app
                                    .prefix2_key
                                    .as_ref()
                                    .map(format_key_binding)
                                    .unwrap_or_default();
                                cached_base_index = app.window_base_index;
                                cached_pred_dim = app.prediction_dimming;
                                cached_status_style = app.status_style.clone();
                                cached_bindings_json = serialize_bindings_json(&app);
                                meta_dirty = false;
                            }
                            let _t_layout = std::time::Instant::now();
                            let layout_json = dump_layout_json_fast(&mut app)?;
                            let _layout_ms = _t_layout.elapsed().as_micros();
                            combined_buf.clear();
                            let ss_escaped = json_escape_string(&cached_status_style);
                            let sl_expanded =
                                json_escape_string(&expand_format(&app.status_left, &app));
                            let sr_expanded =
                                json_escape_string(&expand_format(&app.status_right, &app));
                            let pbs_escaped = json_escape_string(&app.pane_border_style);
                            let pabs_escaped = json_escape_string(&app.pane_active_border_style);
                            let pbs_status_escaped = json_escape_string(&app.pane_border_status);
                            let pbf_escaped = json_escape_string(&app.pane_border_format);
                            let sus_escaped = json_escape_string(&app.status_unfocused_style);
                            let wsf_escaped = json_escape_string(&app.window_status_format);
                            let wscf_escaped =
                                json_escape_string(&app.window_status_current_format);
                            let wss_escaped = json_escape_string(&app.window_status_separator);
                            let ws_style_escaped = json_escape_string(&app.window_status_style);
                            let wsc_style_escaped =
                                json_escape_string(&app.window_status_current_style);
                            let mode_style_escaped = json_escape_string(&app.mode_style);
                            let status_position_escaped = json_escape_string(&app.status_position);
                            let status_justify_escaped = json_escape_string(&app.status_justify);
                            // Build status_format JSON array for multi-line status bar
                            let status_format_json = {
                                let mut sf = String::from("[");
                                for (i, fmt_str) in app.status_format.iter().enumerate() {
                                    if i > 0 {
                                        sf.push(',');
                                    }
                                    sf.push('"');
                                    sf.push_str(&json_escape_string(&expand_format(fmt_str, &app)));
                                    sf.push('"');
                                }
                                sf.push(']');
                                sf
                            };
                            let cursor_style_code = crate::rendering::configured_cursor_code();
                            let _ = std::fmt::Write::write_fmt(&mut combined_buf, format_args!(
                        "{{\"layout\":{},\"windows\":{},\"prefix\":\"{}\",\"prefix2\":\"{}\",\"tree\":{},\"base_index\":{},\"prediction_dimming\":{},\"status_style\":\"{}\",\"status_left\":\"{}\",\"status_right\":\"{}\",\"pane_border_style\":\"{}\",\"pane_active_border_style\":\"{}\",\"wsf\":\"{}\",\"wscf\":\"{}\",\"wss\":\"{}\",\"ws_style\":\"{}\",\"wsc_style\":\"{}\",\"clock_mode\":{},\"bindings\":{},\"defaults_suppressed\":{},\"status_left_length\":{},\"status_right_length\":{},\"status_lines\":{},\"status_format\":{},\"mode_style\":\"{}\",\"status_position\":\"{}\",\"status_justify\":\"{}\",\"cursor_style_code\":{},\"status_visible\":{},\"repeat_time\":{},\"zoomed\":{},\"pane_border_status\":\"{}\",\"pane_border_format\":\"{}\",\"status_unfocused_style\":\"{}\",\"sync_input\":{}}}",
                        layout_json, cached_windows_json, cached_prefix_str, cached_prefix2_str, cached_tree_json, cached_base_index, cached_pred_dim, ss_escaped, sl_expanded, sr_expanded, pbs_escaped, pabs_escaped, wsf_escaped, wscf_escaped, wss_escaped, ws_style_escaped, wsc_style_escaped,
                        matches!(app.mode, Mode::ClockMode), cached_bindings_json, app.defaults_suppressed,
                        app.status_left_length, app.status_right_length, app.status_lines, status_format_json,
                        mode_style_escaped, status_position_escaped, status_justify_escaped,
                        cursor_style_code, app.status_visible, app.repeat_time_ms,
                        app.windows.get(app.active_idx).is_some_and(|w| w.zoom_saved.is_some()),
                        pbs_status_escaped, pbf_escaped, sus_escaped, app.sync_input,
                    ));
                            // Inject overlay state (popup, menu, confirm, display_panes)
                            {
                                let overlay_json = serialize_overlay_json(&app);
                                if !overlay_json.is_empty() && combined_buf.ends_with('}') {
                                    combined_buf.pop();
                                    combined_buf.push_str(&overlay_json);
                                    combined_buf.push('}');
                                }
                            }
                            cached_dump_state.clear();
                            cached_dump_state.push_str(&combined_buf);
                            // Inject one-shot clipboard data for OSC 52 delivery to
                            // the client.  Only the *response* includes this field;
                            // the cached copy does not, so subsequent NC frames won't
                            // re-trigger clipboard emission on the client.
                            if let Some(clip_text) = app.clipboard_osc52.take() {
                                let clip_b64 = base64_encode(&clip_text);
                                // Replace trailing '}' with the extra field
                                if combined_buf.ends_with('}') {
                                    combined_buf.pop();
                                    combined_buf.push_str(",\"clipboard_osc52\":\"");
                                    combined_buf.push_str(&clip_b64);
                                    combined_buf.push_str("\"}");
                                }
                            }
                            cached_data_version = combined_data_version(&app);
                            state_dirty = false;
                            // Timing log: dump-state build time
                            if std::env::var("PSMUX_LATENCY_LOG").unwrap_or_default() == "1" {
                                let total_us = _t_layout.elapsed().as_micros();
                                use std::io::Write as _;
                                static SRV_LOG: std::sync::OnceLock<
                                    std::sync::Mutex<std::fs::File>,
                                > = std::sync::OnceLock::new();
                                let log = SRV_LOG.get_or_init(|| {
                                    let p = std::path::PathBuf::from(
                                        std::env::var("USERPROFILE")
                                            .unwrap_or_else(|_| "C:\\Users\\gj".into()),
                                    )
                                    .join("psmux_server_latency.log");
                                    std::sync::Mutex::new(
                                        std::fs::File::create(p).expect("create latency log"),
                                    )
                                });
                                if let Ok(mut f) = log.lock() {
                                    let _ = writeln!(
                                        f,
                                        "[SRV] dump: layout={}us total={}us json_len={}",
                                        _layout_ms,
                                        total_us,
                                        combined_buf.len()
                                    );
                                }
                            }
                            // Push the newly-built frame to ALL persistent clients so
                            // that other attached sessions see the update immediately,
                            // even if they are idle and not polling dump-state.
                            // Without this, the DumpState handler clears state_dirty,
                            // and the bottom-of-loop push section never fires for frames
                            // already served to the requesting client.
                            if crate::debug_log::memory_log_enabled() {
                                let in_copy =
                                    matches!(app.mode, Mode::CopyMode | Mode::CopySearch { .. });
                                if in_copy {
                                    crate::debug_log::memory_log(
                                        "dumpstate",
                                        &format!(
                                            "DumpState frame in COPY MODE: combined_buf={} cached_dump_state={} (cap: combined={} cached={})",
                                            crate::debug_log::format_bytes(combined_buf.len() as u64),
                                            crate::debug_log::format_bytes(cached_dump_state.len() as u64),
                                            crate::debug_log::format_bytes(combined_buf.capacity() as u64),
                                            crate::debug_log::format_bytes(cached_dump_state.capacity() as u64),
                                        ),
                                    );
                                }
                            }
                            crate::types::push_frame(&cached_dump_state);
                            let _ = resp.send(combined_buf.clone());
                        }
                        CtrlReq::SendText(s) => {
                            app.status_message = None;
                            send_text_to_active(&mut app, &s)?;
                            echo_pending_until = Some(Instant::now());
                        }
                        CtrlReq::SendKey(k) => {
                            app.status_message = None;
                            send_key_to_active(&mut app, &k)?;
                            echo_pending_until = Some(Instant::now());
                        }
                        CtrlReq::SendPaste(s) => {
                            send_paste_to_active(&mut app, &s)?;
                            echo_pending_until = Some(Instant::now());
                        }
                        CtrlReq::ZoomPane => {
                            toggle_zoom(&mut app);
                            meta_dirty = true;
                            hook_event = Some("after-resize-pane");
                        }
                        CtrlReq::PrefixBegin => {
                            app.client_prefix_active = true;
                            state_dirty = true;
                        }
                        CtrlReq::PrefixEnd => {
                            app.client_prefix_active = false;
                            state_dirty = true;
                        }
                        CtrlReq::CopyEnter => {
                            enter_copy_mode(&mut app);
                        }
                        CtrlReq::CopyEnterPageUp => {
                            enter_copy_mode(&mut app);
                            let half = app
                                .windows
                                .get(app.active_idx)
                                .and_then(|w| active_pane(&w.root, &w.active_path))
                                .map(|p| p.last_rows as usize)
                                .unwrap_or(20);
                            scroll_copy_up(&mut app, half);
                        }
                        CtrlReq::ClockMode => {
                            app.mode = Mode::ClockMode;
                            state_dirty = true;
                        }
                        CtrlReq::CopyMove(dx, dy) => {
                            move_copy_cursor(&mut app, dx, dy);
                        }
                        CtrlReq::CopyAnchor => {
                            if let Some((r, c)) = current_prompt_pos(&mut app) {
                                app.copy_anchor = Some((r, c));
                                app.copy_anchor_scroll_offset = app.copy_scroll_offset;
                                app.copy_pos = Some((r, c));
                            }
                        }
                        CtrlReq::CopyYank => {
                            let _ = yank_selection(&mut app);
                            exit_copy_mode(&mut app);
                        }
                        CtrlReq::CopyRectToggle => {
                            app.copy_selection_mode = match app.copy_selection_mode {
                                crate::types::SelectionMode::Rect => {
                                    crate::types::SelectionMode::Char
                                }
                                _ => crate::types::SelectionMode::Rect,
                            };
                        }
                        CtrlReq::ClientSize(cid, w, h) => {
                            app.client_sizes.insert(cid, (w, h));
                            app.latest_client_id = Some(cid);
                            let (ew, eh) = compute_effective_client_size(&app).unwrap_or((w, h));
                            app.last_window_area = Rect {
                                x: 0,
                                y: 0,
                                width: ew,
                                height: eh,
                            };
                            resize_all_panes(&mut app);
                            // Respawn warm pane at the new terminal dimensions so
                            // the next new-window gets a pane whose parser grid
                            // already matches the display — no resize reflow needed,
                            // prompt appears pixel-perfect on the first frame.
                            let need_respawn = app
                                .warm_pane
                                .as_ref()
                                .is_none_or(|wp| wp.rows != eh || wp.cols != ew);
                            if need_respawn {
                                if let Some(mut old) = app.warm_pane.take() {
                                    old.child.kill().ok();
                                }
                                if let Ok(wp) = spawn_warm_pane(&*pty_system, &mut app) {
                                    app.warm_pane = Some(wp);
                                }
                            }
                        }
                        CtrlReq::FocusPaneCmd(pid) => {
                            let old_path = app.windows[app.active_idx].active_path.clone();
                            switch_with_copy_save(&mut app, |app| {
                                focus_pane_by_id(app, pid);
                            });
                            let new_path = app.windows[app.active_idx].active_path.clone();
                            if new_path != old_path {
                                helpers::send_focus_events(&mut app, &old_path, &new_path);
                                unzoom_if_zoomed(&mut app);
                            }
                            meta_dirty = true;
                        }
                        CtrlReq::FocusWindowCmd(wid) => {
                            switch_with_copy_save(&mut app, |app| {
                                if let Some(idx) = find_window_index_by_id(app, wid) {
                                    app.active_idx = idx;
                                }
                            });
                            resize_all_panes(&mut app);
                            meta_dirty = true;
                        }
                        CtrlReq::MouseDown(_, x, y) => {
                            if app.mouse_enabled {
                                remote_mouse_down(&mut app, x, y);
                                state_dirty = true;
                                meta_dirty = true;
                                echo_pending_until = Some(Instant::now());
                            }
                        }
                        CtrlReq::MouseDownRight(_, x, y) => {
                            if app.mouse_enabled {
                                remote_mouse_button(&mut app, x, y, 2, true);
                                state_dirty = true;
                                echo_pending_until = Some(Instant::now());
                            }
                        }
                        CtrlReq::MouseDownMiddle(_, x, y) => {
                            if app.mouse_enabled {
                                remote_mouse_button(&mut app, x, y, 1, true);
                                state_dirty = true;
                                echo_pending_until = Some(Instant::now());
                            }
                        }
                        CtrlReq::MouseDrag(_, x, y) => {
                            if app.mouse_enabled {
                                remote_mouse_drag(&mut app, x, y);
                                state_dirty = true;
                                echo_pending_until = Some(Instant::now());
                            }
                        }
                        CtrlReq::MouseUp(_, x, y) => {
                            if app.mouse_enabled {
                                remote_mouse_up(&mut app, x, y);
                                state_dirty = true;
                                echo_pending_until = Some(Instant::now());
                            }
                        }
                        CtrlReq::MouseUpRight(_, x, y) => {
                            if app.mouse_enabled {
                                remote_mouse_button(&mut app, x, y, 2, false);
                                state_dirty = true;
                                echo_pending_until = Some(Instant::now());
                            }
                        }
                        CtrlReq::MouseUpMiddle(_, x, y) => {
                            if app.mouse_enabled {
                                remote_mouse_button(&mut app, x, y, 1, false);
                                state_dirty = true;
                                echo_pending_until = Some(Instant::now());
                            }
                        }
                        CtrlReq::MouseMove(_, x, y) => {
                            if app.mouse_enabled {
                                remote_mouse_motion(&mut app, x, y);
                                state_dirty = true;
                                echo_pending_until = Some(Instant::now());
                            }
                        }
                        CtrlReq::ScrollUp(_, x, y) => {
                            if app.mouse_enabled {
                                // Track scroll-while-dirty: if state was already dirty
                                // from a previous scroll, we're queuing frames faster
                                // than they can be pushed.
                                if crate::debug_log::memory_log_enabled() && state_dirty {
                                    let in_copy = matches!(
                                        app.mode,
                                        Mode::CopyMode | Mode::CopySearch { .. }
                                    );
                                    if in_copy {
                                        crate::debug_log::memory_log(
                                            "scroll-burst",
                                            "ScrollUp arrived while state_dirty=true in copy mode (frame backlog building)",
                                        );
                                    }
                                }
                                remote_scroll_up(&mut app, x, y);
                                state_dirty = true;
                                echo_pending_until = Some(Instant::now());
                            }
                        }
                        CtrlReq::ScrollDown(_, x, y) => {
                            if app.mouse_enabled {
                                if crate::debug_log::memory_log_enabled() && state_dirty {
                                    let in_copy = matches!(
                                        app.mode,
                                        Mode::CopyMode | Mode::CopySearch { .. }
                                    );
                                    if in_copy {
                                        crate::debug_log::memory_log(
                                            "scroll-burst",
                                            "ScrollDown arrived while state_dirty=true in copy mode (frame backlog building)",
                                        );
                                    }
                                }
                                remote_scroll_down(&mut app, x, y);
                                state_dirty = true;
                                echo_pending_until = Some(Instant::now());
                            }
                        }
                        CtrlReq::NextWindow => {
                            if !app.windows.is_empty() {
                                switch_with_copy_save(&mut app, |app| {
                                    app.last_window_idx = app.active_idx;
                                    app.active_idx = (app.active_idx + 1) % app.windows.len();
                                });
                                resize_all_panes(&mut app);
                            }
                            meta_dirty = true;
                            hook_event = Some("after-select-window");
                        }
                        CtrlReq::PrevWindow => {
                            if !app.windows.is_empty() {
                                switch_with_copy_save(&mut app, |app| {
                                    app.last_window_idx = app.active_idx;
                                    app.active_idx = (app.active_idx + app.windows.len() - 1)
                                        % app.windows.len();
                                });
                                resize_all_panes(&mut app);
                            }
                            meta_dirty = true;
                            hook_event = Some("after-select-window");
                        }
                        CtrlReq::RenameWindow(name) => {
                            let win = &mut app.windows[app.active_idx];
                            win.name = name;
                            win.manual_rename = true;
                            meta_dirty = true;
                            hook_event = Some("after-rename-window");
                        }
                        CtrlReq::ListWindows(resp) => {
                            helpers::propagate_osc_titles(&mut app);
                            let json = list_windows_json(&app)?;
                            let _ = resp.send(json);
                        }
                        CtrlReq::ListWindowsTmux(resp) => {
                            helpers::propagate_osc_titles(&mut app);
                            let text = list_windows_tmux(&app);
                            let _ = resp.send(text);
                        }
                        CtrlReq::ListWindowsFormat(resp, fmt) => {
                            helpers::propagate_osc_titles(&mut app);
                            let text = format_list_windows(&app, &fmt);
                            let _ = resp.send(text);
                        }
                        CtrlReq::ListTree(resp) => {
                            let json = list_tree_json(&app)?;
                            let _ = resp.send(json);
                        }
                        CtrlReq::ToggleSync => {
                            app.sync_input = !app.sync_input;
                        }
                        CtrlReq::SetPaneTitle(title) => {
                            let win = &mut app.windows[app.active_idx];
                            if let Some(p) = active_pane_mut(&mut win.root, &win.active_path) {
                                p.title = title;
                                p.title_locked = true;
                            }
                        }
                        CtrlReq::SetPaneStyle(style) => {
                            // Per-pane styling (e.g. "bg=default,fg=blue") matching
                            // tmux's `-P` flag which sets window-style + window-active-style.
                            // Store on the pane for API compatibility; ConPTY rendering
                            // doesn't support per-pane fg/bg tinting yet.
                            let win = &mut app.windows[app.active_idx];
                            if let Some(p) = active_pane_mut(&mut win.root, &win.active_path) {
                                p.pane_style = Some(style);
                            }
                        }
                        CtrlReq::SetPaneOption(key, value) => {
                            let win = &mut app.windows[app.active_idx];
                            if let Some(p) = active_pane_mut(&mut win.root, &win.active_path) {
                                p.metadata.insert(key, value);
                            }
                            state_dirty = true;
                        }
                        CtrlReq::ShowPaneOptionValue(resp, key) => {
                            let val = {
                                let win = &app.windows[app.active_idx];
                                active_pane(&win.root, &win.active_path)
                                    .and_then(|p| p.metadata.get(&key).cloned())
                                    .unwrap_or_default()
                            };
                            let _ = resp.send(val);
                        }
                        CtrlReq::SendKeys(keys, literal) => {
                            let in_copy =
                                matches!(app.mode, Mode::CopyMode | Mode::CopySearch { .. });
                            if in_copy {
                                // In copy/search mode — route through mode-aware handlers
                                if literal {
                                    send_text_to_active(&mut app, &keys)?;
                                } else {
                                    let parts: Vec<&str> = keys.split_whitespace().collect();
                                    for key in parts.iter() {
                                        let key_upper = key.to_uppercase();
                                        let normalized = match key_upper.as_str() {
                                            "ENTER" => "enter",
                                            "TAB" => "tab",
                                            "BTAB" | "BACKTAB" => "btab",
                                            "ESCAPE" | "ESC" => "esc",
                                            "SPACE" => "space",
                                            "BSPACE" | "BACKSPACE" => "backspace",
                                            "UP" => "up",
                                            "DOWN" => "down",
                                            "RIGHT" => "right",
                                            "LEFT" => "left",
                                            "HOME" => "home",
                                            "END" => "end",
                                            "PAGEUP" | "PPAGE" => "pageup",
                                            "PAGEDOWN" | "NPAGE" => "pagedown",
                                            "DELETE" | "DC" => "delete",
                                            "INSERT" | "IC" => "insert",
                                            _ => "",
                                        };
                                        if !normalized.is_empty() {
                                            send_key_to_active(&mut app, normalized)?;
                                        } else if key_upper.starts_with("C-")
                                            || key_upper.starts_with("M-")
                                            || (key_upper.starts_with("F")
                                                && key_upper.len() >= 2
                                                && key_upper[1..]
                                                    .chars()
                                                    .all(|c| c.is_ascii_digit()))
                                        {
                                            send_key_to_active(&mut app, &key.to_lowercase())?;
                                        } else {
                                            // Plain text char — route through send_text_to_active (handles copy mode chars)
                                            send_text_to_active(&mut app, key)?;
                                        }
                                    }
                                }
                            } else if literal {
                                send_text_to_active(&mut app, &keys)?;
                            } else {
                                let parts: Vec<&str> = keys.split_whitespace().collect();
                                for (i, key) in parts.iter().enumerate() {
                                    let key_upper = key.to_uppercase();
                                    let _is_special = matches!(
                                        key_upper.as_str(),
                                        "ENTER"
                                            | "TAB"
                                            | "BTAB"
                                            | "BACKTAB"
                                            | "ESCAPE"
                                            | "ESC"
                                            | "SPACE"
                                            | "BSPACE"
                                            | "BACKSPACE"
                                            | "UP"
                                            | "DOWN"
                                            | "RIGHT"
                                            | "LEFT"
                                            | "HOME"
                                            | "END"
                                            | "PAGEUP"
                                            | "PPAGE"
                                            | "PAGEDOWN"
                                            | "NPAGE"
                                            | "DELETE"
                                            | "DC"
                                            | "INSERT"
                                            | "IC"
                                            | "F1"
                                            | "F2"
                                            | "F3"
                                            | "F4"
                                            | "F5"
                                            | "F6"
                                            | "F7"
                                            | "F8"
                                            | "F9"
                                            | "F10"
                                            | "F11"
                                            | "F12"
                                    ) || key_upper.starts_with("C-")
                                        || key_upper.starts_with("M-")
                                        || key_upper.starts_with("S-");

                                    match key_upper.as_str() {
                                        "ENTER" => send_text_to_active(&mut app, "\r")?,
                                        "TAB" => send_text_to_active(&mut app, "\t")?,
                                        "BTAB" | "BACKTAB" => {
                                            send_text_to_active(&mut app, "\x1b[Z")?
                                        }
                                        "ESCAPE" | "ESC" => send_text_to_active(&mut app, "\x1b")?,
                                        "SPACE" => send_text_to_active(&mut app, " ")?,
                                        "BSPACE" | "BACKSPACE" => {
                                            send_text_to_active(&mut app, "\x7f")?
                                        }
                                        "UP" => send_text_to_active(&mut app, "\x1b[A")?,
                                        "DOWN" => send_text_to_active(&mut app, "\x1b[B")?,
                                        "RIGHT" => send_text_to_active(&mut app, "\x1b[C")?,
                                        "LEFT" => send_text_to_active(&mut app, "\x1b[D")?,
                                        "HOME" => send_text_to_active(&mut app, "\x1b[H")?,
                                        "END" => send_text_to_active(&mut app, "\x1b[F")?,
                                        "PAGEUP" | "PPAGE" => {
                                            send_text_to_active(&mut app, "\x1b[5~")?
                                        }
                                        "PAGEDOWN" | "NPAGE" => {
                                            send_text_to_active(&mut app, "\x1b[6~")?
                                        }
                                        "DELETE" | "DC" => {
                                            send_text_to_active(&mut app, "\x1b[3~")?
                                        }
                                        "INSERT" | "IC" => {
                                            send_text_to_active(&mut app, "\x1b[2~")?
                                        }
                                        "F1" => send_text_to_active(&mut app, "\x1bOP")?,
                                        "F2" => send_text_to_active(&mut app, "\x1bOQ")?,
                                        "F3" => send_text_to_active(&mut app, "\x1bOR")?,
                                        "F4" => send_text_to_active(&mut app, "\x1bOS")?,
                                        "F5" => send_text_to_active(&mut app, "\x1b[15~")?,
                                        "F6" => send_text_to_active(&mut app, "\x1b[17~")?,
                                        "F7" => send_text_to_active(&mut app, "\x1b[18~")?,
                                        "F8" => send_text_to_active(&mut app, "\x1b[19~")?,
                                        "F9" => send_text_to_active(&mut app, "\x1b[20~")?,
                                        "F10" => send_text_to_active(&mut app, "\x1b[21~")?,
                                        "F11" => send_text_to_active(&mut app, "\x1b[23~")?,
                                        "F12" => send_text_to_active(&mut app, "\x1b[24~")?,
                                        // Modifier + special key combos (C-Left, S-Right, C-M-Up, etc.)
                                        // must be checked BEFORE the generic C-x / M-x single-char handlers.
                                        s if crate::input::parse_modified_special_key(s)
                                            .is_some() =>
                                        {
                                            let seq = crate::input::parse_modified_special_key(s)
                                                .unwrap();
                                            send_text_to_active(&mut app, &seq)?;
                                        }
                                        s if s.starts_with("C-M-") || s.starts_with("C-m-") => {
                                            if let Some(c) = key.chars().nth(4) {
                                                let ctrl = (c.to_ascii_lowercase() as u8) & 0x1F;
                                                send_text_to_active(
                                                    &mut app,
                                                    &format!("\x1b{}", ctrl as char),
                                                )?;
                                            }
                                        }
                                        s if s.starts_with("C-") => {
                                            if let Some(c) = s.chars().nth(2) {
                                                let ctrl = (c.to_ascii_lowercase() as u8) & 0x1F;
                                                send_text_to_active(
                                                    &mut app,
                                                    &String::from(ctrl as char),
                                                )?;
                                                // On Windows, writing 0x03 to the PTY pipe doesn't
                                                // generate CTRL_C_EVENT when ENABLE_PROCESSED_INPUT
                                                // is disabled (e.g. after a TUI app).  Fire the real
                                                // signal via the platform helper so detached/headless
                                                // send-keys C-c reliably interrupts processes.
                                                #[cfg(windows)]
                                                if ctrl == 0x03 {
                                                    if let Some(win) =
                                                        app.windows.get_mut(app.active_idx)
                                                    {
                                                        if let Some(p) = active_pane_mut(
                                                            &mut win.root,
                                                            &win.active_path,
                                                        ) {
                                                            if p.child_pid.is_none() {
                                                                p.child_pid = crate::platform::mouse_inject::get_child_pid(&*p.child);
                                                            }
                                                            if let Some(pid) = p.child_pid {
                                                                crate::platform::mouse_inject::send_ctrl_c_event(pid, false);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        s if s.starts_with("M-") => {
                                            if let Some(c) = key.chars().nth(2) {
                                                send_text_to_active(
                                                    &mut app,
                                                    &format!("\x1b{}", c),
                                                )?;
                                            }
                                        }
                                        _ => {
                                            send_text_to_active(&mut app, key)?;
                                            if i + 1 < parts.len() {
                                                let next_upper = parts[i + 1].to_uppercase();
                                                let next_is_special = matches!(
                                                    next_upper.as_str(),
                                                    "ENTER"
                                                        | "TAB"
                                                        | "BTAB"
                                                        | "BACKTAB"
                                                        | "ESCAPE"
                                                        | "ESC"
                                                        | "SPACE"
                                                        | "BSPACE"
                                                        | "BACKSPACE"
                                                        | "UP"
                                                        | "DOWN"
                                                        | "RIGHT"
                                                        | "LEFT"
                                                        | "HOME"
                                                        | "END"
                                                        | "PAGEUP"
                                                        | "PPAGE"
                                                        | "PAGEDOWN"
                                                        | "NPAGE"
                                                        | "DELETE"
                                                        | "DC"
                                                        | "INSERT"
                                                        | "IC"
                                                        | "F1"
                                                        | "F2"
                                                        | "F3"
                                                        | "F4"
                                                        | "F5"
                                                        | "F6"
                                                        | "F7"
                                                        | "F8"
                                                        | "F9"
                                                        | "F10"
                                                        | "F11"
                                                        | "F12"
                                                ) || next_upper
                                                    .starts_with("C-")
                                                    || next_upper.starts_with("M-")
                                                    || next_upper.starts_with("S-");
                                                if !next_is_special {
                                                    send_text_to_active(&mut app, " ")?;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            echo_pending_until = Some(Instant::now());
                        }
                        CtrlReq::SendKeysX(cmd) => {
                            // send-keys -X: dispatch copy-mode commands by name
                            // This is the primary mechanism used by tmux-yank and other plugins
                            let in_copy =
                                matches!(app.mode, Mode::CopyMode | Mode::CopySearch { .. });
                            if !in_copy {
                                // Auto-enter copy mode for commands that require it
                                enter_copy_mode(&mut app);
                            }
                            match cmd.as_str() {
                                "cancel" => {
                                    app.mode = Mode::Passthrough;
                                    app.copy_anchor = None;
                                    app.copy_pos = None;
                                    app.copy_scroll_offset = 0;
                                    let win = &mut app.windows[app.active_idx];
                                    if let Some(p) =
                                        active_pane_mut(&mut win.root, &win.active_path)
                                    {
                                        if let Ok(mut parser) = p.term.lock() {
                                            parser.screen_mut().set_scrollback(0);
                                        }
                                    }
                                }
                                "begin-selection" => {
                                    if let Some((r, c)) = crate::copy_mode::get_copy_pos(&mut app) {
                                        app.copy_anchor = Some((r, c));
                                        app.copy_anchor_scroll_offset = app.copy_scroll_offset;
                                        app.copy_pos = Some((r, c));
                                        app.copy_selection_mode = crate::types::SelectionMode::Char;
                                    }
                                }
                                "select-line" => {
                                    if let Some((r, c)) = crate::copy_mode::get_copy_pos(&mut app) {
                                        app.copy_anchor = Some((r, c));
                                        app.copy_anchor_scroll_offset = app.copy_scroll_offset;
                                        app.copy_pos = Some((r, c));
                                        app.copy_selection_mode = crate::types::SelectionMode::Line;
                                    }
                                }
                                "rectangle-toggle" => {
                                    app.copy_selection_mode = match app.copy_selection_mode {
                                        crate::types::SelectionMode::Rect => {
                                            crate::types::SelectionMode::Char
                                        }
                                        _ => crate::types::SelectionMode::Rect,
                                    };
                                }
                                "copy-selection" => {
                                    let _ = yank_selection(&mut app);
                                }
                                "copy-selection-and-cancel" => {
                                    let _ = yank_selection(&mut app);
                                    app.mode = Mode::Passthrough;
                                    app.copy_scroll_offset = 0;
                                    app.copy_pos = None;
                                }
                                "copy-selection-no-clear" => {
                                    let _ = yank_selection(&mut app);
                                }
                                s if s.starts_with("copy-pipe-and-cancel")
                                    || s.starts_with("copy-pipe") =>
                                {
                                    // copy-pipe[-and-cancel] [command] — yank + pipe to command
                                    let _ = yank_selection(&mut app);
                                    // Extract pipe command from argument if present
                                    let cancel = s.contains("cancel");
                                    let pipe_cmd = cmd
                                        .strip_prefix("copy-pipe-and-cancel")
                                        .or_else(|| cmd.strip_prefix("copy-pipe"))
                                        .unwrap_or("")
                                        .trim();
                                    if !pipe_cmd.is_empty() {
                                        if let Some(text) = app.paste_buffers.first().cloned() {
                                            // Pipe yanked text to the command's stdin
                                            if let Ok(mut child) =
                                                std::process::Command::new(if cfg!(windows) {
                                                    "pwsh"
                                                } else {
                                                    "sh"
                                                })
                                                .args(if cfg!(windows) {
                                                    vec!["-NoProfile", "-Command", pipe_cmd]
                                                } else {
                                                    vec!["-c", pipe_cmd]
                                                })
                                                .stdin(std::process::Stdio::piped())
                                                .stdout(std::process::Stdio::null())
                                                .stderr(std::process::Stdio::null())
                                                .spawn()
                                            {
                                                if let Some(mut stdin) = child.stdin.take() {
                                                    use std::io::Write;
                                                    let _ = stdin.write_all(text.as_bytes());
                                                }
                                                let _ = child.wait();
                                            }
                                        }
                                    }
                                    if cancel {
                                        app.mode = Mode::Passthrough;
                                        app.copy_scroll_offset = 0;
                                        app.copy_pos = None;
                                    }
                                }
                                "cursor-up" => {
                                    move_copy_cursor(&mut app, 0, -1);
                                }
                                "cursor-down" => {
                                    move_copy_cursor(&mut app, 0, 1);
                                }
                                "cursor-left" => {
                                    move_copy_cursor(&mut app, -1, 0);
                                }
                                "cursor-right" => {
                                    move_copy_cursor(&mut app, 1, 0);
                                }
                                "start-of-line" => {
                                    crate::copy_mode::move_to_line_start(&mut app);
                                }
                                "end-of-line" => {
                                    crate::copy_mode::move_to_line_end(&mut app);
                                }
                                "back-to-indentation" => {
                                    crate::copy_mode::move_to_first_nonblank(&mut app);
                                }
                                "next-word" => {
                                    crate::copy_mode::move_word_forward(&mut app);
                                }
                                "previous-word" => {
                                    crate::copy_mode::move_word_backward(&mut app);
                                }
                                "next-word-end" => {
                                    crate::copy_mode::move_word_end(&mut app);
                                }
                                "next-space" => {
                                    crate::copy_mode::move_word_forward_big(&mut app);
                                }
                                "previous-space" => {
                                    crate::copy_mode::move_word_backward_big(&mut app);
                                }
                                "next-space-end" => {
                                    crate::copy_mode::move_word_end_big(&mut app);
                                }
                                "top-line" => {
                                    crate::copy_mode::move_to_screen_top(&mut app);
                                }
                                "middle-line" => {
                                    crate::copy_mode::move_to_screen_middle(&mut app);
                                }
                                "bottom-line" => {
                                    crate::copy_mode::move_to_screen_bottom(&mut app);
                                }
                                "history-top" => {
                                    crate::copy_mode::scroll_to_top(&mut app);
                                }
                                "history-bottom" => {
                                    crate::copy_mode::scroll_to_bottom(&mut app);
                                }
                                "halfpage-up" => {
                                    let half = app
                                        .windows
                                        .get(app.active_idx)
                                        .and_then(|w| active_pane(&w.root, &w.active_path))
                                        .map(|p| (p.last_rows / 2) as usize)
                                        .unwrap_or(10);
                                    scroll_copy_up(&mut app, half);
                                }
                                "halfpage-down" => {
                                    let half = app
                                        .windows
                                        .get(app.active_idx)
                                        .and_then(|w| active_pane(&w.root, &w.active_path))
                                        .map(|p| (p.last_rows / 2) as usize)
                                        .unwrap_or(10);
                                    scroll_copy_down(&mut app, half);
                                }
                                "page-up" => {
                                    scroll_copy_up(&mut app, 20);
                                }
                                "page-down" => {
                                    scroll_copy_down(&mut app, 20);
                                }
                                "scroll-up" => {
                                    scroll_copy_up(&mut app, 1);
                                }
                                "scroll-down" => {
                                    scroll_copy_down(&mut app, 1);
                                }
                                "search-forward" | "search-forward-incremental" => {
                                    app.mode = Mode::CopySearch {
                                        input: String::new(),
                                        forward: true,
                                    };
                                }
                                "search-backward" | "search-backward-incremental" => {
                                    app.mode = Mode::CopySearch {
                                        input: String::new(),
                                        forward: false,
                                    };
                                }
                                "search-again" => {
                                    crate::copy_mode::search_next(&mut app);
                                }
                                "search-reverse" => {
                                    crate::copy_mode::search_prev(&mut app);
                                }
                                "copy-end-of-line" => {
                                    let _ = crate::copy_mode::copy_end_of_line(&mut app);
                                    app.mode = Mode::Passthrough;
                                    app.copy_scroll_offset = 0;
                                    app.copy_pos = None;
                                }
                                "select-word" => {
                                    // Select the word under cursor
                                    crate::copy_mode::move_word_backward(&mut app);
                                    if let Some((r, c)) = crate::copy_mode::get_copy_pos(&mut app) {
                                        app.copy_anchor = Some((r, c));
                                        app.copy_anchor_scroll_offset = app.copy_scroll_offset;
                                        app.copy_selection_mode = crate::types::SelectionMode::Char;
                                    }
                                    crate::copy_mode::move_word_end(&mut app);
                                }
                                "other-end" => {
                                    if let (Some(a), Some(p)) = (app.copy_anchor, app.copy_pos) {
                                        app.copy_anchor = Some(p);
                                        app.copy_anchor_scroll_offset = app.copy_scroll_offset;
                                        app.copy_pos = Some(a);
                                    }
                                }
                                "clear-selection" => {
                                    app.copy_anchor = None;
                                    app.copy_selection_mode = crate::types::SelectionMode::Char;
                                }
                                "append-selection" => {
                                    // Append to existing buffer instead of replacing
                                    let _ = yank_selection(&mut app);
                                    if app.paste_buffers.len() >= 2 {
                                        let appended = format!(
                                            "{}{}",
                                            app.paste_buffers[1], app.paste_buffers[0]
                                        );
                                        app.paste_buffers[0] = appended;
                                    }
                                }
                                "append-selection-and-cancel" => {
                                    let _ = yank_selection(&mut app);
                                    if app.paste_buffers.len() >= 2 {
                                        let appended = format!(
                                            "{}{}",
                                            app.paste_buffers[1], app.paste_buffers[0]
                                        );
                                        app.paste_buffers[0] = appended;
                                    }
                                    app.mode = Mode::Passthrough;
                                    app.copy_scroll_offset = 0;
                                    app.copy_pos = None;
                                }
                                "copy-line" => {
                                    // Select entire current line and yank
                                    if let Some((r, _)) = crate::copy_mode::get_copy_pos(&mut app) {
                                        app.copy_anchor = Some((r, 0));
                                        app.copy_anchor_scroll_offset = app.copy_scroll_offset;
                                        app.copy_selection_mode = crate::types::SelectionMode::Line;
                                        let cols = app
                                            .windows
                                            .get(app.active_idx)
                                            .and_then(|w| active_pane(&w.root, &w.active_path))
                                            .map(|p| p.last_cols)
                                            .unwrap_or(80);
                                        app.copy_pos = Some((r, cols.saturating_sub(1)));
                                        let _ = yank_selection(&mut app);
                                    }
                                    app.mode = Mode::Passthrough;
                                    app.copy_scroll_offset = 0;
                                    app.copy_pos = None;
                                }
                                s if s.starts_with("goto-line") => {
                                    // goto-line <N> — jump to line N in scrollback
                                    let n = s
                                        .strip_prefix("goto-line")
                                        .unwrap_or("")
                                        .trim()
                                        .parse::<u16>()
                                        .unwrap_or(0);
                                    app.copy_pos = Some((n, 0));
                                }
                                "jump-forward" => {
                                    app.copy_find_char_pending = Some(0);
                                }
                                "jump-backward" => {
                                    app.copy_find_char_pending = Some(1);
                                }
                                "jump-to-forward" => {
                                    app.copy_find_char_pending = Some(2);
                                }
                                "jump-to-backward" => {
                                    app.copy_find_char_pending = Some(3);
                                }
                                "jump-again" => {
                                    // Repeat last find-char in same direction
                                    // We'd need to store last char; for now emit the pending
                                }
                                "jump-reverse" => {
                                    // Repeat last find-char in reverse direction
                                }
                                "next-paragraph" => {
                                    crate::copy_mode::move_next_paragraph(&mut app);
                                }
                                "previous-paragraph" => {
                                    crate::copy_mode::move_prev_paragraph(&mut app);
                                }
                                "next-matching-bracket" => {
                                    crate::copy_mode::move_matching_bracket(&mut app);
                                }
                                "stop-selection" => {
                                    // Keep cursor position but stop extending selection
                                    app.copy_anchor = None;
                                }
                                _ => {} // ignore unknown copy-mode commands
                            }
                        }
                        CtrlReq::SelectPane(dir) => {
                            // Auto-unzoom when navigating to another pane (tmux behavior).
                            // For directional nav: unzoom first so compute_rects uses
                            // real geometry, then re-zoom only if focus didn't change.
                            // For other cases: only unzoom if focus actually changes.
                            // (fixes #46)
                            match dir.as_str() {
                                "U" | "D" | "L" | "R" => {
                                    let focus_dir = match dir.as_str() {
                                        "U" => FocusDir::Up,
                                        "D" => FocusDir::Down,
                                        "L" => FocusDir::Left,
                                        _ => FocusDir::Right,
                                    };
                                    let was_zoomed = unzoom_if_zoomed(&mut app);
                                    if was_zoomed {
                                        // Zoom-aware: check for direct neighbor or wrap target (#134).
                                        // Navigate if there's any reachable pane in that direction.
                                        let win = &app.windows[app.active_idx];
                                        let mut rects: Vec<(Vec<usize>, ratatui::layout::Rect)> =
                                            Vec::new();
                                        crate::tree::compute_rects(
                                            &win.root,
                                            app.last_window_area,
                                            &mut rects,
                                        );
                                        let active_idx = rects
                                            .iter()
                                            .position(|(path, _)| *path == win.active_path);
                                        let has_target = if let Some(ai) = active_idx {
                                            let (_, arect) = &rects[ai];
                                            find_best_pane_in_direction(
                                                &rects,
                                                ai,
                                                arect,
                                                focus_dir,
                                                &[],
                                                &[],
                                            )
                                            .or_else(|| {
                                                find_wrap_target(
                                                    &rects,
                                                    ai,
                                                    arect,
                                                    focus_dir,
                                                    &[],
                                                    &[],
                                                )
                                            })
                                            .is_some()
                                        } else {
                                            false
                                        };
                                        if has_target {
                                            let old_path =
                                                app.windows[app.active_idx].active_path.clone();
                                            switch_with_copy_save(&mut app, |app| {
                                                move_focus(app, focus_dir);
                                            });
                                            app.last_pane_path = old_path;
                                        } else {
                                            // No reachable pane (single-pane window) — re-zoom
                                            toggle_zoom(&mut app);
                                        }
                                    } else {
                                        let old_path =
                                            app.windows[app.active_idx].active_path.clone();
                                        switch_with_copy_save(&mut app, |app| {
                                            move_focus(app, focus_dir);
                                        });
                                        if app.windows[app.active_idx].active_path != old_path {
                                            app.last_pane_path = old_path;
                                        }
                                    }
                                }
                                "last" => {
                                    // select-pane -l: switch to last active pane
                                    let old_path = app.windows[app.active_idx].active_path.clone();
                                    switch_with_copy_save(&mut app, |app| {
                                        let win = &mut app.windows[app.active_idx];
                                        if !app.last_pane_path.is_empty() {
                                            let tmp = win.active_path.clone();
                                            win.active_path = app.last_pane_path.clone();
                                            app.last_pane_path = tmp;
                                        }
                                    });
                                    if app.windows[app.active_idx].active_path != old_path {
                                        // Update MRU for the newly focused pane
                                        let win = &mut app.windows[app.active_idx];
                                        if let Some(pid) =
                                            get_active_pane_id(&win.root, &win.active_path)
                                        {
                                            crate::tree::touch_mru(&mut win.pane_mru, pid);
                                        }
                                        unzoom_if_zoomed(&mut app);
                                    }
                                }
                                "mark" => {
                                    // select-pane -m: mark the current pane
                                    let win = &app.windows[app.active_idx];
                                    if let Some(pid) =
                                        get_active_pane_id(&win.root, &win.active_path)
                                    {
                                        app.marked_pane = Some((app.active_idx, pid));
                                    }
                                }
                                "next" => {
                                    // select-pane next: cycle to next pane (like Prefix+o / tmux -t :.+)
                                    let old_path = app.windows[app.active_idx].active_path.clone();
                                    switch_with_copy_save(&mut app, |app| {
                                        let win = &app.windows[app.active_idx];
                                        let mut pane_paths = Vec::new();
                                        let mut path = Vec::new();
                                        collect_pane_paths_server(
                                            &win.root,
                                            &mut path,
                                            &mut pane_paths,
                                        );
                                        if let Some(cur) =
                                            pane_paths.iter().position(|p| *p == win.active_path)
                                        {
                                            let next = (cur + 1) % pane_paths.len();
                                            let new_path = pane_paths[next].clone();
                                            let win = &mut app.windows[app.active_idx];
                                            app.last_pane_path = win.active_path.clone();
                                            win.active_path = new_path;
                                        }
                                    });
                                    if app.windows[app.active_idx].active_path != old_path {
                                        let win = &mut app.windows[app.active_idx];
                                        if let Some(pid) =
                                            get_active_pane_id(&win.root, &win.active_path)
                                        {
                                            crate::tree::touch_mru(&mut win.pane_mru, pid);
                                        }
                                        unzoom_if_zoomed(&mut app);
                                    }
                                }
                                "prev" => {
                                    // select-pane prev: cycle to previous pane (tmux -t :.-)
                                    let old_path = app.windows[app.active_idx].active_path.clone();
                                    switch_with_copy_save(&mut app, |app| {
                                        let win = &app.windows[app.active_idx];
                                        let mut pane_paths = Vec::new();
                                        let mut path = Vec::new();
                                        collect_pane_paths_server(
                                            &win.root,
                                            &mut path,
                                            &mut pane_paths,
                                        );
                                        if let Some(cur) =
                                            pane_paths.iter().position(|p| *p == win.active_path)
                                        {
                                            let prev =
                                                (cur + pane_paths.len() - 1) % pane_paths.len();
                                            let new_path = pane_paths[prev].clone();
                                            let win = &mut app.windows[app.active_idx];
                                            app.last_pane_path = win.active_path.clone();
                                            win.active_path = new_path;
                                        }
                                    });
                                    if app.windows[app.active_idx].active_path != old_path {
                                        let win = &mut app.windows[app.active_idx];
                                        if let Some(pid) =
                                            get_active_pane_id(&win.root, &win.active_path)
                                        {
                                            crate::tree::touch_mru(&mut win.pane_mru, pid);
                                        }
                                        unzoom_if_zoomed(&mut app);
                                    }
                                }
                                "unmark" => {
                                    // select-pane -M: clear the marked pane
                                    app.marked_pane = None;
                                }
                                _ => {}
                            }
                            meta_dirty = true;
                            hook_event = Some("after-select-pane");
                        }
                        CtrlReq::SelectWindow(idx) => {
                            if idx >= app.window_base_index {
                                let internal_idx = idx - app.window_base_index;
                                if internal_idx < app.windows.len()
                                    && internal_idx != app.active_idx
                                {
                                    switch_with_copy_save(&mut app, |app| {
                                        app.last_window_idx = app.active_idx;
                                        app.active_idx = internal_idx;
                                    });
                                    resize_all_panes(&mut app);
                                }
                            }
                            meta_dirty = true;
                            hook_event = Some("after-select-window");
                        }
                        CtrlReq::ListPanes(resp) => {
                            helpers::propagate_osc_titles(&mut app);
                            let mut output = String::new();
                            let win = &app.windows[app.active_idx];
                            fn collect_panes(
                                node: &Node,
                                panes: &mut Vec<(
                                    usize,
                                    u16,
                                    u16,
                                    vt100::MouseProtocolMode,
                                    vt100::MouseProtocolEncoding,
                                    bool,
                                )>,
                            ) {
                                match node {
                                    Node::Leaf(p) => {
                                        let (mode, enc, alt) = match p.term.lock() {
                                            Ok(term) => {
                                                let screen = term.screen();
                                                (
                                                    screen.mouse_protocol_mode(),
                                                    screen.mouse_protocol_encoding(),
                                                    screen.alternate_screen(),
                                                )
                                            }
                                            Err(_) => {
                                                // Mutex poisoned — reader thread panicked.  Use safe defaults.
                                                (
                                                    vt100::MouseProtocolMode::None,
                                                    vt100::MouseProtocolEncoding::Default,
                                                    false,
                                                )
                                            }
                                        };
                                        panes.push((
                                            p.id,
                                            p.last_cols,
                                            p.last_rows,
                                            mode,
                                            enc,
                                            alt,
                                        ));
                                    }
                                    Node::Split { children, .. } => {
                                        for c in children {
                                            collect_panes(c, panes);
                                        }
                                    }
                                }
                            }
                            let mut panes = Vec::new();
                            collect_panes(&win.root, &mut panes);
                            let active_pane_id =
                                crate::tree::get_active_pane_id(&win.root, &win.active_path);
                            for (pos, (id, cols, rows, _mode, _enc, _alt)) in
                                panes.iter().enumerate()
                            {
                                let idx = pos + app.pane_base_index;
                                let active_marker = if active_pane_id == Some(*id) {
                                    " (active)"
                                } else {
                                    ""
                                };
                                output.push_str(&format!(
                                    "{}: [{}x{}] [history {}/{}, 0 bytes] %{}{}\n",
                                    idx,
                                    cols,
                                    rows,
                                    app.history_limit,
                                    app.history_limit,
                                    id,
                                    active_marker
                                ));
                            }
                            let _ = resp.send(output);
                        }
                        CtrlReq::ListPanesFormat(resp, fmt) => {
                            helpers::propagate_osc_titles(&mut app);
                            let text = format_list_panes(&app, &fmt, app.active_idx);
                            let _ = resp.send(text);
                        }
                        CtrlReq::ListAllPanes(resp) => {
                            let mut output = String::new();
                            fn collect_all_panes(node: &Node, panes: &mut Vec<(usize, u16, u16)>) {
                                match node {
                                    Node::Leaf(p) => {
                                        panes.push((p.id, p.last_cols, p.last_rows));
                                    }
                                    Node::Split { children, .. } => {
                                        for c in children {
                                            collect_all_panes(c, panes);
                                        }
                                    }
                                }
                            }
                            for (wi, win) in app.windows.iter().enumerate() {
                                let mut panes = Vec::new();
                                collect_all_panes(&win.root, &mut panes);
                                for (id, cols, rows) in panes {
                                    output.push_str(&format!(
                                        "{}:{}: %{} [{}x{}]\n",
                                        app.session_name,
                                        wi + app.window_base_index,
                                        id,
                                        cols,
                                        rows
                                    ));
                                }
                            }
                            let _ = resp.send(output);
                        }
                        CtrlReq::ListAllPanesFormat(resp, fmt) => {
                            let mut lines = Vec::new();
                            for wi in 0..app.windows.len() {
                                lines.push(format_list_panes(&app, &fmt, wi));
                            }
                            let _ = resp.send(lines.join("\n"));
                        }
                        CtrlReq::ListSessionsJson(resp) => {
                            let info = crate::util::SessionJsonInfo {
                                name: app.session_name.clone(),
                                windows: app.windows.len(),
                                attached: app.attached_clients > 0,
                                created: app.created_at.to_rfc3339(),
                            };
                            let json =
                                serde_json::to_string(&[info]).unwrap_or_else(|_| "[]".to_string());
                            let _ = resp.send(json);
                        }
                        CtrlReq::ListPanesJson(resp, all) => {
                            helpers::propagate_osc_titles(&mut app);
                            #[allow(clippy::type_complexity)]
                            fn collect_panes_json(
                                node: &Node,
                                panes: &mut Vec<(usize, u16, u16, String, Option<u32>, String)>,
                            ) {
                                match node {
                                    Node::Leaf(p) => {
                                        let current_path = if let Some(pid) = p.child_pid {
                                            crate::platform::process_info::get_foreground_cwd(pid)
                                                .unwrap_or_default()
                                        } else {
                                            String::new()
                                        };
                                        panes.push((
                                            p.id,
                                            p.last_cols,
                                            p.last_rows,
                                            p.title.clone(),
                                            p.child_pid,
                                            current_path,
                                        ));
                                    }
                                    Node::Split { children, .. } => {
                                        for c in children {
                                            collect_panes_json(c, panes);
                                        }
                                    }
                                }
                            }
                            let mut result: Vec<crate::util::PaneJsonInfo> = Vec::new();
                            let win_range: Vec<usize> = if all {
                                (0..app.windows.len()).collect()
                            } else {
                                vec![app.active_idx]
                            };
                            for wi in win_range {
                                let win = &app.windows[wi];
                                let active_pane_id =
                                    crate::tree::get_active_pane_id(&win.root, &win.active_path);
                                let mut panes = Vec::new();
                                collect_panes_json(&win.root, &mut panes);
                                for (pos, (id, cols, rows, title, pid, current_path)) in
                                    panes.into_iter().enumerate()
                                {
                                    result.push(crate::util::PaneJsonInfo {
                                        pane_id: format!("%{}", id),
                                        window_index: wi + app.window_base_index,
                                        pane_index: pos + app.pane_base_index,
                                        width: cols,
                                        height: rows,
                                        active: active_pane_id == Some(id),
                                        pid,
                                        current_path,
                                        title,
                                    });
                                }
                            }
                            let json =
                                serde_json::to_string(&result).unwrap_or_else(|_| "[]".to_string());
                            let _ = resp.send(json);
                        }
                        CtrlReq::ListWindowsJson(resp) => {
                            helpers::propagate_osc_titles(&mut app);
                            let layout_names = [
                                "even-horizontal",
                                "even-vertical",
                                "main-horizontal",
                                "main-vertical",
                                "tiled",
                            ];
                            let mut result: Vec<crate::util::WindowJsonInfo> = Vec::new();
                            for (i, w) in app.windows.iter().enumerate() {
                                let pane_count = crate::tree::count_panes(&w.root);
                                let (width, height) =
                                    if let Some(p) = active_pane(&w.root, &w.active_path) {
                                        (p.last_cols, p.last_rows)
                                    } else {
                                        (120, 30)
                                    };
                                let layout = layout_names
                                    .get(w.layout_index)
                                    .unwrap_or(&"custom")
                                    .to_string();
                                result.push(crate::util::WindowJsonInfo {
                                    index: i + app.window_base_index,
                                    name: w.name.clone(),
                                    layout,
                                    active: i == app.active_idx,
                                    panes: pane_count,
                                    width,
                                    height,
                                });
                            }
                            let json =
                                serde_json::to_string(&result).unwrap_or_else(|_| "[]".to_string());
                            let _ = resp.send(json);
                        }
                        CtrlReq::CapturePaneJson(resp) => {
                            let pane_id = get_active_pane_id(
                                &app.windows[app.active_idx].root,
                                &app.windows[app.active_idx].active_path,
                            )
                            .unwrap_or(0);
                            let content = capture_active_pane_text(&mut app)?.unwrap_or_default();
                            let info = crate::util::CapturePaneJson {
                                pane_id: format!("%{}", pane_id),
                                content,
                            };
                            let json =
                                serde_json::to_string(&info).unwrap_or_else(|_| "{}".to_string());
                            let _ = resp.send(json);
                        }
                        CtrlReq::KillWindow => {
                            if app.windows.len() > 1 {
                                let mut win = app.windows.remove(app.active_idx);
                                kill_all_children(&mut win.root);
                                if app.active_idx >= app.windows.len() {
                                    app.active_idx = app.windows.len() - 1;
                                }
                            } else {
                                // Last window: kill all children; reaper will detect empty session and exit
                                kill_all_children(&mut app.windows[0].root);
                            }
                            hook_event = Some("window-closed");
                        }
                        CtrlReq::KillSession => {
                            // Remove port/key/version/pipe files FIRST so clients see the
                            // session as gone immediately, then kill processes.
                            let home = env::var("USERPROFILE")
                                .or_else(|_| env::var("HOME"))
                                .unwrap_or_default();
                            let regpath =
                                format!("{}\\.psmux\\{}.port", home, app.port_file_base());
                            let keypath = format!("{}\\.psmux\\{}.key", home, app.port_file_base());
                            let verpath =
                                format!("{}\\.psmux\\{}.version", home, app.port_file_base());
                            let pipepath =
                                format!("{}\\.psmux\\{}.pipe", home, app.port_file_base());
                            let _ = std::fs::remove_file(&regpath);
                            let _ = std::fs::remove_file(&keypath);
                            let _ = std::fs::remove_file(&verpath);
                            let _ = std::fs::remove_file(&pipepath);
                            crate::types::shutdown_persistent_streams();
                            // Kill all child processes using a single process snapshot
                            tree::kill_all_children_batch(&mut app.windows);
                            // Kill warm pane's child (process::exit skips Drop)
                            if let Some(mut wp) = app.warm_pane.take() {
                                wp.child.kill().ok();
                            }
                            // Kill orphaned warm servers if this was the last
                            // non-warm session (#120, #138).
                            if !is_warm_server(&app) {
                                let ns = app.socket_name.as_deref().map(|l| format!("{l}__"));
                                crate::session::kill_warm_servers(ns.as_deref());
                            }
                            #[cfg(feature = "mycel")]
                            crate::mycel::publish_pane_event(
                                crate::mycel::topics::SESSION_KILLED,
                                &serde_json::json!({
                                    "session_name": app.session_name,
                                    "client_id": crate::mycel::mycel_client_id().unwrap_or("unknown"),
                                }),
                            );
                            // TerminateProcess is synchronous on Windows — processes
                            // are already dead.  Minimal delay for OS handle cleanup.
                            std::thread::sleep(std::time::Duration::from_millis(10));
                            std::process::exit(0);
                        }
                        CtrlReq::HasSession(resp) => {
                            let _ = resp.send(true);
                        }
                        CtrlReq::WaitPane(pane_id, resp) => {
                            // Check if the pane exists and has already exited
                            let mut found = false;
                            let mut already_exited = false;
                            let mut exit_code: i32 = 0;
                            for win in app.windows.iter_mut() {
                                if let Some(path) = crate::tree::find_path_by_id(&win.root, pane_id)
                                {
                                    found = true;
                                    if let Some(p) =
                                        crate::tree::active_pane_mut(&mut win.root, &path)
                                    {
                                        if p.dead {
                                            already_exited = true;
                                            // Try to get exit code from child
                                            if let Ok(Some(status)) = p.child.try_wait() {
                                                exit_code = status.exit_code() as i32;
                                            }
                                        } else if let Ok(Some(status)) = p.child.try_wait() {
                                            already_exited = true;
                                            exit_code = status.exit_code() as i32;
                                        }
                                    }
                                    break;
                                }
                            }
                            if !found || already_exited {
                                // Pane not found or already dead: respond immediately
                                let _ = resp.send(exit_code);
                            } else {
                                // Pane still alive: queue the waiter
                                app.wait_pane_queue.push((pane_id, resp));
                            }
                        }
                        CtrlReq::QueryPaneReady(pane_id, resp) => {
                            let mut dv = 0u64;
                            let mut lot = 0u64;
                            for win in app.windows.iter() {
                                if let Some(path) = crate::tree::find_path_by_id(&win.root, pane_id)
                                {
                                    if let Some(p) = crate::tree::active_pane(&win.root, &path) {
                                        dv = p
                                            .data_version
                                            .load(std::sync::atomic::Ordering::Acquire);
                                        lot = p
                                            .last_output_time
                                            .load(std::sync::atomic::Ordering::Acquire);
                                    }
                                    break;
                                }
                            }
                            let _ = resp.send((dv, lot));
                        }
                        CtrlReq::RenameSession(name) => {
                            let _old_session_name = app.session_name.clone();
                            let home = env::var("USERPROFILE")
                                .or_else(|_| env::var("HOME"))
                                .unwrap_or_default();
                            let old_path =
                                format!("{}\\.psmux\\{}.port", home, app.port_file_base());
                            let old_keypath =
                                format!("{}\\.psmux\\{}.key", home, app.port_file_base());
                            // Compute new port file base with socket_name prefix
                            let new_base = if let Some(ref sn) = app.socket_name {
                                format!("{}__{}", sn, name)
                            } else {
                                name.clone()
                            };
                            let new_path = format!("{}\\.psmux\\{}.port", home, new_base);
                            let new_keypath = format!("{}\\.psmux\\{}.key", home, new_base);
                            if let Some(port) = app.control_port {
                                let _ = std::fs::remove_file(&old_path);
                                let _ = std::fs::write(&new_path, port.to_string());
                                if let Ok(key) = std::fs::read_to_string(&old_keypath) {
                                    let _ = std::fs::remove_file(&old_keypath);
                                    let _ = std::fs::write(&new_keypath, key);
                                }
                                // Rename .pipe discovery file so backend socket remains discoverable
                                let old_pipepath =
                                    format!("{}\\.psmux\\{}.pipe", home, app.port_file_base());
                                let new_pipepath = format!("{}\\.psmux\\{}.pipe", home, new_base);
                                if std::path::Path::new(&old_pipepath).exists() {
                                    let _ = std::fs::rename(&old_pipepath, &new_pipepath);
                                }
                            }
                            app.session_name = name;
                            #[cfg(feature = "mycel")]
                            crate::mycel::publish_pane_event(
                                crate::mycel::topics::SESSION_RENAMED,
                                &serde_json::json!({
                                    "session_name": app.session_name,
                                    "old_name": _old_session_name,
                                    "client_id": crate::mycel::mycel_client_id().unwrap_or("unknown"),
                                }),
                            );
                            // Update env so run-shell/hooks from this server target the new name
                            crate::util::set_env("PSMUX_TARGET_SESSION", app.port_file_base());
                            hook_event = Some("after-rename-session");
                        }
                        CtrlReq::ClaimSession(name, client_cwd, resp) => {
                            // Same as RenameSession but with a synchronous response
                            // so the CLI knows the rename completed before attaching.
                            let home = env::var("USERPROFILE")
                                .or_else(|_| env::var("HOME"))
                                .unwrap_or_default();
                            let old_path =
                                format!("{}\\.psmux\\{}.port", home, app.port_file_base());
                            let old_keypath =
                                format!("{}\\.psmux\\{}.key", home, app.port_file_base());
                            let old_verpath =
                                format!("{}\\.psmux\\{}.version", home, app.port_file_base());
                            let new_base = if let Some(ref sn) = app.socket_name {
                                format!("{}__{}", sn, name)
                            } else {
                                name.clone()
                            };
                            let new_path = format!("{}\\.psmux\\{}.port", home, new_base);
                            let new_keypath = format!("{}\\.psmux\\{}.key", home, new_base);
                            let new_verpath = format!("{}\\.psmux\\{}.version", home, new_base);
                            if let Some(port) = app.control_port {
                                let _ = std::fs::remove_file(&old_path);
                                let _ = std::fs::write(&new_path, port.to_string());
                                if let Ok(key) = std::fs::read_to_string(&old_keypath) {
                                    let _ = std::fs::remove_file(&old_keypath);
                                    let _ = std::fs::write(&new_keypath, key);
                                }
                                // Rename version stamp alongside port/key
                                let _ = std::fs::remove_file(&old_verpath);
                                let _ = std::fs::write(
                                    &new_verpath,
                                    crate::types::build_version_stamp(),
                                );
                                // Rename .pipe discovery file so backend socket remains discoverable
                                let old_pipepath =
                                    format!("{}\\.psmux\\{}.pipe", home, app.port_file_base());
                                let new_pipepath = format!("{}\\.psmux\\{}.pipe", home, new_base);
                                if std::path::Path::new(&old_pipepath).exists() {
                                    let _ = std::fs::rename(&old_pipepath, &new_pipepath);
                                }
                            }
                            app.session_name = name;
                            // Update env so run-shell/hooks from this server target the new name
                            crate::util::set_env("PSMUX_TARGET_SESSION", app.port_file_base());
                            // Re-load user config so the claimed session reflects the
                            // current config file.  The warm server loaded config at
                            // its own startup, but the user may have changed their
                            // config since then (or the warm server was spawned by a
                            // different session with a different PSMUX_CONFIG_FILE).
                            load_config(&mut app);
                            // Update shared aliases after config reload
                            if let Ok(mut w) = shared_aliases_main.write() {
                                *w = app.command_aliases.clone();
                            }
                            // Honour the client's working directory: the warm server
                            // was spawned from a previous session whose CWD may differ
                            // from where the user ran `psmux` now.  Inject `cd` into
                            // the active pane with squelch to hide the command flash.
                            if let Some(ref cwd) = client_cwd {
                                let cwd_path = std::path::Path::new(cwd);
                                if cwd_path.is_dir() {
                                    let server_cwd_differs = env::current_dir()
                                        .map(|cur| cur != cwd_path)
                                        .unwrap_or(true);
                                    if server_cwd_differs {
                                        env::set_current_dir(cwd_path).ok();
                                        if let Some(win) = app.windows.last_mut() {
                                            if let Some(p) =
                                                active_pane_mut(&mut win.root, &win.active_path)
                                            {
                                                use std::io::Write as _;
                                                let escaped = cwd.replace('\'', "''");
                                                let clear =
                                                    if cfg!(windows) { "cls" } else { "clear" };
                                                let cd_cmd =
                                                    format!(" cd '{}'; {}\r", escaped, clear);
                                                // Tell the vt100 parser to watch for the
                                                // next screen-clear event (CSI 2J/3J).
                                                if let Ok(mut parser) = p.term.lock() {
                                                    parser
                                                        .screen_mut()
                                                        .set_squelch_clear_pending(true);
                                                }
                                                p.squelch_until = Some(
                                                    Instant::now() + Duration::from_millis(500),
                                                );
                                                let _ = p.writer.write_all(cd_cmd.as_bytes());
                                                let _ = p.writer.flush();
                                            }
                                        }
                                    }
                                }
                            }
                            meta_dirty = true;
                            state_dirty = true;
                            let _ = resp.send("OK\n".to_string());
                            // Spawn a replacement warm server for the NEXT new-session
                            spawn_warm_server(&app);
                            hook_event = Some("after-rename-session");
                        }
                        CtrlReq::SwapPane(dir) => {
                            // tmux: swap-pane without -Z permanently unzooms (#82)
                            unzoom_if_zoomed(&mut app);
                            match dir.as_str() {
                                "U" => {
                                    swap_pane(&mut app, FocusDir::Up);
                                }
                                "D" => {
                                    swap_pane(&mut app, FocusDir::Down);
                                }
                                _ => {
                                    swap_pane(&mut app, FocusDir::Down);
                                }
                            }
                            hook_event = Some("after-swap-pane");
                        }
                        CtrlReq::ResizePane(dir, amount) => {
                            unzoom_if_zoomed(&mut app);
                            match dir.as_str() {
                                "U" | "D" => {
                                    resize_pane_vertical(
                                        &mut app,
                                        if dir == "U" {
                                            -(amount as i16)
                                        } else {
                                            amount as i16
                                        },
                                    );
                                }
                                "L" | "R" => {
                                    resize_pane_horizontal(
                                        &mut app,
                                        if dir == "L" {
                                            -(amount as i16)
                                        } else {
                                            amount as i16
                                        },
                                    );
                                }
                                _ => {}
                            }
                            hook_event = Some("after-resize-pane");
                        }
                        CtrlReq::SetBuffer(content) => {
                            app.paste_buffers.insert(0, content);
                            if app.paste_buffers.len() > 10 {
                                app.paste_buffers.pop();
                            }
                        }
                        CtrlReq::ListBuffers(resp) => {
                            let mut output = String::new();
                            for (i, buf) in app.paste_buffers.iter().enumerate() {
                                let preview: String = buf.chars().take(50).collect();
                                output.push_str(&format!(
                                    "buffer{}: {} bytes: \"{}\"\n",
                                    i,
                                    buf.len(),
                                    preview
                                ));
                            }
                            let _ = resp.send(output);
                        }
                        CtrlReq::ListBuffersFormat(resp, fmt) => {
                            let mut output = Vec::new();
                            for (i, _buf) in app.paste_buffers.iter().enumerate() {
                                set_buffer_idx_override(Some(i));
                                output.push(expand_format(&fmt, &app));
                                set_buffer_idx_override(None);
                            }
                            let _ = resp.send(output.join("\n"));
                        }
                        CtrlReq::ShowBuffer(resp) => {
                            let content = app.paste_buffers.first().cloned().unwrap_or_default();
                            let _ = resp.send(content);
                        }
                        CtrlReq::ShowBufferAt(resp, idx) => {
                            let content = app.paste_buffers.get(idx).cloned().unwrap_or_default();
                            let _ = resp.send(content);
                        }
                        CtrlReq::DeleteBuffer => {
                            if !app.paste_buffers.is_empty() {
                                app.paste_buffers.remove(0);
                            }
                        }
                        CtrlReq::DisplayMessage(resp, fmt, target_pane_idx, _) => {
                            // Propagate OSC titles so #{pane_title} reflects latest state
                            helpers::propagate_osc_titles(&mut app);
                            let result = if let Some(pane_idx) = target_pane_idx {
                                // -t targeting: evaluate format for the specific pane
                                // using PANE_POS_OVERRIDE so #{pane_active} reflects
                                // the REAL active pane, not the target (#113)
                                crate::format::expand_format_for_pane(
                                    &fmt,
                                    &app,
                                    app.active_idx,
                                    pane_idx,
                                )
                            } else {
                                expand_format(&fmt, &app)
                            };
                            let _ = resp.send(result);
                        }
                        CtrlReq::LastWindow => {
                            if app.windows.len() > 1 && app.last_window_idx < app.windows.len() {
                                switch_with_copy_save(&mut app, |app| {
                                    std::mem::swap(&mut app.active_idx, &mut app.last_window_idx);
                                });
                            }
                            meta_dirty = true;
                            hook_event = Some("after-select-window");
                        }
                        CtrlReq::LastPane => {
                            switch_with_copy_save(&mut app, |app| {
                                let win = &mut app.windows[app.active_idx];
                                if !app.last_pane_path.is_empty()
                                    && path_exists(&win.root, &app.last_pane_path)
                                {
                                    let tmp = win.active_path.clone();
                                    win.active_path = app.last_pane_path.clone();
                                    app.last_pane_path = tmp;
                                } else if !win.active_path.is_empty() {
                                    let last = win.active_path.last_mut();
                                    if let Some(idx) = last {
                                        *idx = (*idx + 1) % 2;
                                    }
                                }
                            });
                            meta_dirty = true;
                        }
                        CtrlReq::RotateWindow(reverse) => {
                            rotate_panes(&mut app, reverse);
                            hook_event = Some("after-rotate-window");
                        }
                        CtrlReq::DisplayPanes => {
                            // Setup display_map and enter PaneChooser mode
                            let win = &app.windows[app.active_idx];
                            let mut rects: Vec<(Vec<usize>, ratatui::layout::Rect)> = Vec::new();
                            crate::tree::compute_rects(&win.root, app.last_window_area, &mut rects);
                            app.display_map.clear();
                            for (i, (path, _)) in rects.into_iter().enumerate() {
                                if i >= 10 {
                                    break;
                                }
                                let digit = (i + app.pane_base_index) % 10;
                                app.display_map.push((digit, path));
                            }
                            app.mode = Mode::PaneChooser {
                                opened_at: std::time::Instant::now(),
                            };
                            state_dirty = true;
                        }
                        CtrlReq::DisplayPaneSelect(digit) => {
                            // User pressed a digit during display-panes overlay: select the matching pane
                            if let Some((_, path)) =
                                app.display_map.iter().find(|(d, _)| *d == digit)
                            {
                                let new_path = path.clone();
                                let old_path = app.windows[app.active_idx].active_path.clone();
                                app.windows[app.active_idx].active_path = new_path;
                                if app.windows[app.active_idx].active_path != old_path {
                                    app.last_pane_path = old_path;
                                }
                            }
                            app.mode = Mode::Passthrough;
                            state_dirty = true;
                            meta_dirty = true;
                        }
                        CtrlReq::BreakPane => {
                            unzoom_if_zoomed(&mut app);
                            break_pane_to_window(&mut app);
                            hook_event = Some("after-break-pane");
                            meta_dirty = true;
                        }
                        CtrlReq::JoinPane(target_win) => {
                            unzoom_if_zoomed(&mut app);
                            // Real join-pane: extract active pane from current window and
                            // graft it as a vertical split into the target window.
                            let src_idx = app.active_idx;
                            if target_win < app.windows.len() && target_win != src_idx {
                                let src_path = app.windows[src_idx].active_path.clone();
                                let src_root = std::mem::replace(
                                    &mut app.windows[src_idx].root,
                                    Node::Split {
                                        kind: LayoutKind::Horizontal,
                                        sizes: vec![],
                                        children: vec![],
                                    },
                                );
                                let (remaining, extracted) =
                                    tree::extract_node(src_root, &src_path);
                                if let Some(pane_node) = extracted {
                                    let src_empty = remaining.is_none();
                                    if let Some(rem) = remaining {
                                        app.windows[src_idx].root = rem;
                                        app.windows[src_idx].active_path =
                                            tree::first_leaf_path(&app.windows[src_idx].root);
                                    }
                                    // Adjust target index if source window will be removed
                                    let tgt = if src_empty && target_win > src_idx {
                                        target_win - 1
                                    } else {
                                        target_win
                                    };
                                    if src_empty {
                                        app.windows.remove(src_idx);
                                        if app.active_idx >= app.windows.len() {
                                            app.active_idx = app.windows.len().saturating_sub(1);
                                        }
                                    }
                                    // Graft pane into target window
                                    if tgt < app.windows.len() {
                                        let tgt_path = app.windows[tgt].active_path.clone();
                                        tree::replace_leaf_with_split(
                                            &mut app.windows[tgt].root,
                                            &tgt_path,
                                            LayoutKind::Vertical,
                                            pane_node,
                                        );
                                        app.active_idx = tgt;
                                    }
                                    resize_all_panes(&mut app);
                                    meta_dirty = true;
                                    hook_event = Some("after-join-pane");
                                } else {
                                    // Extraction failed — restore
                                    if let Some(rem) = remaining {
                                        app.windows[src_idx].root = rem;
                                    }
                                }
                            }
                        }
                        CtrlReq::RespawnPane(kill) => {
                            // Failure here (e.g. "pane still active" without -k)
                            // must not tear down the server — log and continue.
                            match respawn_active_pane(&mut app, Some(&*pty_system), kill) {
                                Ok(()) => {
                                    hook_event = Some("after-respawn-pane");
                                }
                                Err(e) => {
                                    eprintln!("psmux respawn-pane: {}", e);
                                }
                            }
                        }
                        CtrlReq::BindKey(table_name, key, command, repeat) => {
                            if let Some(kc) = parse_key_string(&key) {
                                let kc = normalize_key_for_binding(kc);
                                // Support `\;` chaining in server-side bind-key
                                let sub_cmds = crate::config::split_chained_commands_pub(&command);
                                let action = if sub_cmds.len() > 1 {
                                    Some(Action::CommandChain(sub_cmds))
                                } else {
                                    parse_command_to_action(&command)
                                };
                                if let Some(act) = action {
                                    let table = app.key_tables.entry(table_name).or_default();
                                    table.retain(|b| b.key != kc);
                                    table.push(Bind {
                                        key: kc,
                                        action: act,
                                        repeat,
                                    });
                                }
                            }
                            meta_dirty = true;
                            state_dirty = true;
                        }
                        CtrlReq::UnbindKey(key) => {
                            if let Some(kc) = parse_key_string(&key) {
                                let kc = normalize_key_for_binding(kc);
                                for table in app.key_tables.values_mut() {
                                    table.retain(|b| b.key != kc);
                                }
                            }
                            meta_dirty = true;
                            state_dirty = true;
                        }
                        CtrlReq::UnbindAllInTable(table_name) => {
                            app.key_tables.remove(&table_name);
                            if table_name == "prefix" {
                                app.defaults_suppressed = true;
                            }
                            meta_dirty = true;
                            state_dirty = true;
                        }
                        CtrlReq::ListKeys(resp) => {
                            // Build list-keys output from the canonical help module
                            let user_iter =
                                app.key_tables.iter().flat_map(|(table_name, binds)| {
                                    binds.iter().map(move |bind| {
                                        let key_str = format_key_binding(&bind.key);
                                        let action_str = format_action(&bind.action);
                                        (table_name.as_str(), key_str, action_str, bind.repeat)
                                    })
                                });
                            let output =
                                help::build_list_keys_output(user_iter, app.defaults_suppressed);
                            let _ = resp.send(output);
                        }
                        CtrlReq::SetOption(option, value) => {
                            apply_set_option(&mut app, &option, &value, false);
                            // Update shared aliases if command-alias changed
                            if option == "command-alias" {
                                if let Ok(mut map) = shared_aliases_main.write() {
                                    *map = app.command_aliases.clone();
                                }
                            }
                            meta_dirty = true;
                            state_dirty = true;
                        }
                        CtrlReq::SetOptionQuiet(option, value, quiet, only_if_unset) => {
                            if only_if_unset && app.user_set_options.contains(&option) {
                                // -o on already-set option: no-op (P0.2)
                                meta_dirty = true;
                                state_dirty = true;
                                continue;
                            }
                            let old_shell = app.default_shell.clone();
                            apply_set_option(&mut app, &option, &value, quiet);
                            app.user_set_options.insert(option.clone());
                            // If default-shell changed, kill the warm pane so the next
                            // new-window spawns the correct shell (fixes #99).
                            if app.default_shell != old_shell {
                                if let Some(mut wp) = app.warm_pane.take() {
                                    wp.child.kill().ok();
                                }
                            }
                            // Update shared aliases if command-alias changed
                            if option == "command-alias" {
                                if let Ok(mut map) = shared_aliases_main.write() {
                                    *map = app.command_aliases.clone();
                                }
                            }
                            meta_dirty = true;
                            state_dirty = true;
                        }
                        CtrlReq::SetOptionUnset(option) => {
                            // Remove from user_set_options so a subsequent -o can set again (P0.2).
                            app.user_set_options.remove(&option);
                            // Reset option to default or remove @user-option
                            if option.starts_with('@') {
                                app.user_options.remove(&option);
                            } else {
                                match option.as_str() {
                                    "status-left" => {
                                        app.status_left = "psmux:#I".to_string();
                                    }
                                    "status-right" => {
                                        app.status_right = "#{?window_bigger,[#{window_offset_x}#,#{window_offset_y}] ,}\"#{=21:pane_title}\" %H:%M %d-%b-%y".to_string();
                                    }
                                    "mouse" => {
                                        app.mouse_enabled = true;
                                    }
                                    "escape-time" => {
                                        app.escape_time_ms = 500;
                                    }
                                    "history-limit" => {
                                        app.history_limit = 2000;
                                    }
                                    "display-time" => {
                                        app.display_time_ms = 750;
                                    }
                                    "mode-keys" => {
                                        app.mode_keys = "emacs".to_string();
                                    }
                                    "status" => {
                                        app.status_visible = true;
                                    }
                                    "status-position" => {
                                        app.status_position = "bottom".to_string();
                                    }
                                    "status-style" => {
                                        app.status_style = String::new();
                                    }
                                    "renumber-windows" => {
                                        app.renumber_windows = false;
                                    }
                                    "remain-on-exit" => {
                                        app.remain_on_exit = false;
                                    }
                                    "destroy-unattached" => {
                                        app.destroy_unattached = false;
                                    }
                                    "exit-empty" => {
                                        app.exit_empty = true;
                                    }
                                    "automatic-rename" => {
                                        app.automatic_rename = true;
                                    }
                                    "pane-border-style" => {
                                        app.pane_border_style = String::new();
                                    }
                                    "pane-active-border-style" => {
                                        app.pane_active_border_style = "fg=green".to_string();
                                    }
                                    "pane-border-status" => {
                                        app.pane_border_status = "top".to_string();
                                    }
                                    "pane-border-format" => {
                                        app.pane_border_format =
                                            "#{pane_index}: #{pane_title}".to_string();
                                    }
                                    "status-unfocused-style" => {
                                        app.status_unfocused_style = String::new();
                                    }
                                    "window-status-format" => {
                                        app.window_status_format =
                                            "#I:#W#{?window_flags,#{window_flags}, }".to_string();
                                    }
                                    "window-status-current-format" => {
                                        app.window_status_current_format =
                                            "#I:#W#{?window_flags,#{window_flags}, }".to_string();
                                    }
                                    "window-status-separator" => {
                                        app.window_status_separator = " ".to_string();
                                    }
                                    "cursor-style" => {
                                        crate::util::set_env("PSMUX_CURSOR_STYLE", "bar");
                                    }
                                    "cursor-blink" => {
                                        crate::util::set_env("PSMUX_CURSOR_BLINK", "1");
                                    }
                                    _ => {}
                                }
                            }
                        }
                        CtrlReq::SetOptionAppend(option, value) => {
                            // Append to existing option value
                            if option.starts_with('@') {
                                let existing =
                                    app.user_options.get(&option).cloned().unwrap_or_default();
                                app.user_options
                                    .insert(option, format!("{}{}", existing, value));
                            } else {
                                match option.as_str() {
                                    "status-left" => {
                                        app.status_left.push_str(&value);
                                    }
                                    "status-right" => {
                                        app.status_right.push_str(&value);
                                    }
                                    "status-style" => {
                                        app.status_style.push_str(&value);
                                    }
                                    "pane-border-style" => {
                                        app.pane_border_style.push_str(&value);
                                    }
                                    "pane-active-border-style" => {
                                        app.pane_active_border_style.push_str(&value);
                                    }
                                    "window-status-format" => {
                                        app.window_status_format.push_str(&value);
                                    }
                                    "window-status-current-format" => {
                                        app.window_status_current_format.push_str(&value);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        CtrlReq::ShowOptions(resp) => {
                            let mut output = String::new();
                            output.push_str(&format!(
                                "prefix {}\n",
                                format_key_binding(&app.prefix_key)
                            ));
                            if let Some(ref p2) = app.prefix2_key {
                                output.push_str(&format!("prefix2 {}\n", format_key_binding(p2)));
                            }
                            output.push_str(&format!("base-index {}\n", app.window_base_index));
                            output.push_str(&format!("pane-base-index {}\n", app.pane_base_index));
                            output.push_str(&format!("escape-time {}\n", app.escape_time_ms));
                            output.push_str(&format!(
                                "mouse {}\n",
                                if app.mouse_enabled { "on" } else { "off" }
                            ));
                            output.push_str(&format!(
                                "status {}\n",
                                if app.status_visible { "on" } else { "off" }
                            ));
                            output.push_str(&format!("status-position {}\n", app.status_position));
                            output.push_str(&format!("status-left \"{}\"\n", app.status_left));
                            output.push_str(&format!("status-right \"{}\"\n", app.status_right));
                            output.push_str(&format!("history-limit {}\n", app.history_limit));
                            output.push_str(&format!("display-time {}\n", app.display_time_ms));
                            output.push_str(&format!(
                                "display-panes-time {}\n",
                                app.display_panes_time_ms
                            ));
                            output.push_str(&format!("mode-keys {}\n", app.mode_keys));
                            output.push_str(&format!(
                                "focus-events {}\n",
                                if app.focus_events { "on" } else { "off" }
                            ));
                            output.push_str(&format!(
                                "renumber-windows {}\n",
                                if app.renumber_windows { "on" } else { "off" }
                            ));
                            output.push_str(&format!(
                                "automatic-rename {}\n",
                                if app.automatic_rename { "on" } else { "off" }
                            ));
                            output.push_str(&format!(
                                "monitor-activity {}\n",
                                if app.monitor_activity { "on" } else { "off" }
                            ));
                            output.push_str(&format!(
                                "synchronize-panes {}\n",
                                if app.sync_input { "on" } else { "off" }
                            ));
                            output.push_str(&format!(
                                "remain-on-exit {}\n",
                                if app.remain_on_exit { "on" } else { "off" }
                            ));
                            output.push_str(&format!(
                                "destroy-unattached {}\n",
                                if app.destroy_unattached { "on" } else { "off" }
                            ));
                            output.push_str(&format!(
                                "exit-empty {}\n",
                                if app.exit_empty { "on" } else { "off" }
                            ));
                            output.push_str(&format!(
                                "set-titles {}\n",
                                if app.set_titles { "on" } else { "off" }
                            ));
                            if !app.set_titles_string.is_empty() {
                                output.push_str(&format!(
                                    "set-titles-string \"{}\"\n",
                                    app.set_titles_string
                                ));
                            }
                            output.push_str(&format!(
                                "prediction-dimming {}\n",
                                if app.prediction_dimming { "on" } else { "off" }
                            ));
                            output.push_str(&format!(
                                "cursor-style {}\n",
                                std::env::var("PSMUX_CURSOR_STYLE")
                                    .unwrap_or_else(|_| "bar".to_string())
                            ));
                            output.push_str(&format!(
                                "cursor-blink {}\n",
                                if std::env::var("PSMUX_CURSOR_BLINK")
                                    .unwrap_or_else(|_| "1".to_string())
                                    != "0"
                                {
                                    "on"
                                } else {
                                    "off"
                                }
                            ));
                            if !app.default_shell.is_empty() {
                                output.push_str(&format!("default-shell {}\n", app.default_shell));
                            }
                            output.push_str(&format!(
                                "word-separators \"{}\"\n",
                                app.word_separators
                            ));
                            if !app.pane_border_style.is_empty() {
                                output.push_str(&format!(
                                    "pane-border-style \"{}\"\n",
                                    app.pane_border_style
                                ));
                            }
                            if !app.pane_active_border_style.is_empty() {
                                output.push_str(&format!(
                                    "pane-active-border-style \"{}\"\n",
                                    app.pane_active_border_style
                                ));
                            }
                            if !app.pane_border_status.is_empty() {
                                output.push_str(&format!(
                                    "pane-border-status \"{}\"\n",
                                    app.pane_border_status
                                ));
                            }
                            if !app.pane_border_format.is_empty() {
                                output.push_str(&format!(
                                    "pane-border-format \"{}\"\n",
                                    app.pane_border_format
                                ));
                            }
                            if !app.status_unfocused_style.is_empty() {
                                output.push_str(&format!(
                                    "status-unfocused-style \"{}\"\n",
                                    app.status_unfocused_style
                                ));
                            }
                            if !app.status_style.is_empty() {
                                output
                                    .push_str(&format!("status-style \"{}\"\n", app.status_style));
                            }
                            if !app.status_left_style.is_empty() {
                                output.push_str(&format!(
                                    "status-left-style \"{}\"\n",
                                    app.status_left_style
                                ));
                            }
                            if !app.status_right_style.is_empty() {
                                output.push_str(&format!(
                                    "status-right-style \"{}\"\n",
                                    app.status_right_style
                                ));
                            }
                            output.push_str(&format!("status-interval {}\n", app.status_interval));
                            output.push_str(&format!("status-justify {}\n", app.status_justify));
                            output.push_str(&format!(
                                "window-status-format \"{}\"\n",
                                app.window_status_format
                            ));
                            output.push_str(&format!(
                                "window-status-current-format \"{}\"\n",
                                app.window_status_current_format
                            ));
                            if !app.window_status_style.is_empty() {
                                output.push_str(&format!(
                                    "window-status-style \"{}\"\n",
                                    app.window_status_style
                                ));
                            }
                            if !app.window_status_current_style.is_empty() {
                                output.push_str(&format!(
                                    "window-status-current-style \"{}\"\n",
                                    app.window_status_current_style
                                ));
                            }
                            if !app.window_status_activity_style.is_empty() {
                                output.push_str(&format!(
                                    "window-status-activity-style \"{}\"\n",
                                    app.window_status_activity_style
                                ));
                            }
                            if !app.message_style.is_empty() {
                                output.push_str(&format!(
                                    "message-style \"{}\"\n",
                                    app.message_style
                                ));
                            }
                            if !app.message_command_style.is_empty() {
                                output.push_str(&format!(
                                    "message-command-style \"{}\"\n",
                                    app.message_command_style
                                ));
                            }
                            if !app.mode_style.is_empty() {
                                output.push_str(&format!("mode-style \"{}\"\n", app.mode_style));
                            }
                            // Include @user-options (used by plugins)
                            for (key, val) in &app.user_options {
                                output.push_str(&format!("{} \"{}\"\n", key, val));
                            }
                            // New options
                            output.push_str(&format!("main-pane-width {}\n", app.main_pane_width));
                            output
                                .push_str(&format!("main-pane-height {}\n", app.main_pane_height));
                            output.push_str(&format!(
                                "status-left-length {}\n",
                                app.status_left_length
                            ));
                            output.push_str(&format!(
                                "status-right-length {}\n",
                                app.status_right_length
                            ));
                            output.push_str(&format!("window-size {}\n", app.window_size));
                            output.push_str(&format!(
                                "allow-passthrough {}\n",
                                app.allow_passthrough
                            ));
                            output.push_str(&format!("set-clipboard {}\n", app.set_clipboard));
                            if !app.copy_command.is_empty() {
                                output
                                    .push_str(&format!("copy-command \"{}\"\n", app.copy_command));
                            }
                            for (alias, expansion) in &app.command_aliases {
                                output.push_str(&format!(
                                    "command-alias \"{}={}\"\n",
                                    alias, expansion
                                ));
                            }
                            output.push_str(&format!("warm-pool-size {}\n", app.warm_pool_size));
                            let _ = resp.send(output);
                        }
                        CtrlReq::SourceFile(path) => {
                            // Reset defaults_suppressed so the flag reflects the
                            // CURRENT config. If the reloaded config still has
                            // unbind-key -a, parsing will set it back to true.
                            app.defaults_suppressed = false;
                            // Use config helper for standard source-file behavior (-F support,
                            // nested parse context). Keep direct glob handling for wildcard sources.
                            let is_format_expand =
                                path.starts_with("-F ") || path.starts_with("-F\t");
                            let path_for_glob = if is_format_expand {
                                path[3..].trim()
                            } else {
                                &path
                            };
                            if !is_format_expand
                                && (path_for_glob.contains('*') || path_for_glob.contains('?'))
                            {
                                let expanded = if path_for_glob.starts_with('~') {
                                    let home = env::var("USERPROFILE")
                                        .or_else(|_| env::var("HOME"))
                                        .unwrap_or_default();
                                    path_for_glob.replacen('~', &home, 1)
                                } else {
                                    path_for_glob.to_string()
                                };
                                if let Ok(entries) = glob::glob(&expanded) {
                                    for entry in entries.flatten() {
                                        if let Ok(contents) = std::fs::read_to_string(&entry) {
                                            parse_config_content(&mut app, &contents);
                                        }
                                    }
                                }
                            } else if path.ends_with(".json") {
                                // JSON layout file — parse and apply
                                match crate::layout::load_layout_file(&path) {
                                    Ok(layout) => {
                                        if let Err(e) = crate::layout::apply_layout_file(
                                            &mut app,
                                            &*pty_system,
                                            layout,
                                        ) {
                                            crate::debug_log::server_log(
                                                "source-file",
                                                &format!("Layout apply error: {}", e),
                                            );
                                        } else {
                                            crate::debug_log::server_log(
                                                "source-file",
                                                &format!("Applied layout file: {}", path),
                                            );
                                            resize_all_panes(&mut app);
                                            meta_dirty = true;
                                            crate::resurrection::save_snapshot(&app);
                                        }
                                    }
                                    Err(e) => {
                                        crate::debug_log::server_log(
                                            "source-file",
                                            &format!(
                                                "Failed to load layout file '{}': {}",
                                                path, e
                                            ),
                                        );
                                    }
                                }
                            } else {
                                crate::config::source_file(&mut app, &path);
                            }
                        }
                        CtrlReq::MoveWindow(target) => {
                            if let Some(t) = target {
                                if t < app.windows.len() && app.active_idx != t {
                                    let win = app.windows.remove(app.active_idx);
                                    let insert_idx = if t > app.active_idx { t - 1 } else { t };
                                    app.windows.insert(insert_idx.min(app.windows.len()), win);
                                    app.active_idx = insert_idx.min(app.windows.len() - 1);
                                }
                            }
                        }
                        CtrlReq::SwapWindow(target) => {
                            if target < app.windows.len() && app.active_idx != target {
                                app.windows.swap(app.active_idx, target);
                            }
                        }
                        CtrlReq::LinkWindow(_target) => {}
                        CtrlReq::UnlinkWindow => {
                            if app.windows.len() > 1 {
                                let mut win = app.windows.remove(app.active_idx);
                                kill_all_children(&mut win.root);
                                if app.active_idx >= app.windows.len() {
                                    app.active_idx = app.windows.len() - 1;
                                }
                            }
                        }
                        CtrlReq::FindWindow(resp, pattern) => {
                            let mut output = String::new();
                            for (i, win) in app.windows.iter().enumerate() {
                                if win.name.contains(&pattern) {
                                    output.push_str(&format!(
                                        "{}: {} []\n",
                                        i + app.window_base_index,
                                        win.name
                                    ));
                                }
                            }
                            let _ = resp.send(output);
                        }
                        CtrlReq::MovePane(target_win) => {
                            // move-pane is an alias for join-pane
                            let src_idx = app.active_idx;
                            if target_win < app.windows.len() && target_win != src_idx {
                                let src_path = app.windows[src_idx].active_path.clone();
                                let src_root = std::mem::replace(
                                    &mut app.windows[src_idx].root,
                                    Node::Split {
                                        kind: LayoutKind::Horizontal,
                                        sizes: vec![],
                                        children: vec![],
                                    },
                                );
                                let (remaining, extracted) =
                                    tree::extract_node(src_root, &src_path);
                                if let Some(pane_node) = extracted {
                                    let src_empty = remaining.is_none();
                                    if let Some(rem) = remaining {
                                        app.windows[src_idx].root = rem;
                                        app.windows[src_idx].active_path =
                                            tree::first_leaf_path(&app.windows[src_idx].root);
                                    }
                                    let tgt = if src_empty && target_win > src_idx {
                                        target_win - 1
                                    } else {
                                        target_win
                                    };
                                    if src_empty {
                                        app.windows.remove(src_idx);
                                        if app.active_idx >= app.windows.len() {
                                            app.active_idx = app.windows.len().saturating_sub(1);
                                        }
                                    }
                                    if tgt < app.windows.len() {
                                        let tgt_path = app.windows[tgt].active_path.clone();
                                        tree::replace_leaf_with_split(
                                            &mut app.windows[tgt].root,
                                            &tgt_path,
                                            LayoutKind::Vertical,
                                            pane_node,
                                        );
                                        app.active_idx = tgt;
                                    }
                                    resize_all_panes(&mut app);
                                    meta_dirty = true;
                                } else if let Some(rem) = remaining {
                                    app.windows[src_idx].root = rem;
                                }
                            }
                        }
                        CtrlReq::PipePane(cmd, stdin, stdout, toggle) => {
                            let win = &app.windows[app.active_idx];
                            let pane_id =
                                get_active_pane_id(&win.root, &win.active_path).unwrap_or(0);
                            let has_existing = app.pipe_panes.iter().any(|p| p.pane_id == pane_id);

                            if cmd.is_empty() {
                                // No command: close any existing pipe on this pane
                                if let Some(idx) =
                                    app.pipe_panes.iter().position(|p| p.pane_id == pane_id)
                                {
                                    if let Some(ref mut proc) = app.pipe_panes[idx].process {
                                        let _ = proc.kill();
                                    }
                                    app.pipe_panes.remove(idx);
                                }
                            } else if toggle && has_existing {
                                // -o flag with existing pipe: close it (toggle off), don't start new
                                if let Some(idx) =
                                    app.pipe_panes.iter().position(|p| p.pane_id == pane_id)
                                {
                                    if let Some(ref mut proc) = app.pipe_panes[idx].process {
                                        let _ = proc.kill();
                                    }
                                    app.pipe_panes.remove(idx);
                                }
                            } else {
                                // Close any existing pipe first (replace)
                                if let Some(idx) =
                                    app.pipe_panes.iter().position(|p| p.pane_id == pane_id)
                                {
                                    if let Some(ref mut proc) = app.pipe_panes[idx].process {
                                        let _ = proc.kill();
                                    }
                                    app.pipe_panes.remove(idx);
                                }
                                // Start new pipe
                                #[cfg(windows)]
                                let process = std::process::Command::new("pwsh")
                                    .args(["-NoProfile", "-Command", &cmd])
                                    .stdin(if stdout {
                                        std::process::Stdio::piped()
                                    } else {
                                        std::process::Stdio::null()
                                    })
                                    .stdout(if stdin {
                                        std::process::Stdio::piped()
                                    } else {
                                        std::process::Stdio::null()
                                    })
                                    .stderr(std::process::Stdio::null())
                                    .spawn()
                                    .ok();
                                #[cfg(not(windows))]
                                let process = std::process::Command::new("sh")
                                    .args(["-c", &cmd])
                                    .stdin(if stdout {
                                        std::process::Stdio::piped()
                                    } else {
                                        std::process::Stdio::null()
                                    })
                                    .stdout(if stdin {
                                        std::process::Stdio::piped()
                                    } else {
                                        std::process::Stdio::null()
                                    })
                                    .stderr(std::process::Stdio::null())
                                    .spawn()
                                    .ok();

                                app.pipe_panes.push(PipePaneState {
                                    pane_id,
                                    process,
                                    stdin,
                                    stdout,
                                });
                            }
                        }
                        CtrlReq::SelectLayout(layout) => {
                            unzoom_if_zoomed(&mut app);
                            apply_layout(&mut app, &layout);
                            state_dirty = true;
                            crate::resurrection::save_snapshot(&app);
                        }
                        CtrlReq::NextLayout => {
                            unzoom_if_zoomed(&mut app);
                            cycle_layout(&mut app);
                            state_dirty = true;
                        }
                        CtrlReq::ListClients(resp) => {
                            let mut output = String::new();
                            output.push_str(&format!(
                                "/dev/pts/0: {}: {} [{}x{}] (utf8)\n",
                                app.session_name,
                                app.windows[app.active_idx].name,
                                app.last_window_area.width,
                                app.last_window_area.height
                            ));
                            let _ = resp.send(output);
                        }
                        CtrlReq::SwitchClient(_target) => {}
                        CtrlReq::SwitchClientTable(table) => {
                            app.current_key_table = Some(table);
                            state_dirty = true;
                        }
                        CtrlReq::ListCommands(resp) => {
                            let cmds = TMUX_COMMANDS.join("\n");
                            let _ = resp.send(cmds);
                        }
                        CtrlReq::LockClient => {}
                        CtrlReq::RefreshClient => {
                            state_dirty = true;
                            meta_dirty = true;
                        }
                        CtrlReq::SuspendClient => {}
                        CtrlReq::CopyModePageUp => {
                            enter_copy_mode(&mut app);
                            move_copy_cursor(&mut app, 0, -20);
                        }
                        CtrlReq::ClearHistory => {
                            let win = &mut app.windows[app.active_idx];
                            if let Some(p) = active_pane_mut(&mut win.root, &win.active_path) {
                                if let Ok(mut parser) = p.term.lock() {
                                    *parser = vt100::Parser::new(
                                        p.last_rows,
                                        p.last_cols,
                                        app.history_limit,
                                    );
                                }
                            }
                        }
                        CtrlReq::SaveBuffer(path) => {
                            if let Some(content) = app.paste_buffers.first() {
                                let _ = std::fs::write(&path, content);
                            }
                        }
                        CtrlReq::LoadBuffer(path) => {
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                app.paste_buffers.insert(0, content);
                                if app.paste_buffers.len() > 10 {
                                    app.paste_buffers.pop();
                                }
                            }
                        }
                        CtrlReq::SetEnvironment(key, value) => {
                            app.environment.insert(key.clone(), value.clone());
                            crate::util::set_env(&key, &value);
                            // Kill the warm pane and respawn so it picks up the new
                            // env var at process level — avoids PTY echo (#137).
                            if app.warm_pane.is_some() {
                                if let Some(mut old_wp) = app.warm_pane.take() {
                                    old_wp.child.kill().ok();
                                }
                                match spawn_warm_pane(&*pty_system, &mut app) {
                                    Ok(new_wp) => {
                                        app.warm_pane = Some(new_wp);
                                    }
                                    Err(e) => {
                                        eprintln!("psmux: warm pane respawn (SetEnv) failed: {e}");
                                    }
                                }
                            }
                        }
                        CtrlReq::UnsetEnvironment(key) => {
                            app.environment.remove(&key);
                            crate::util::remove_env(&key);
                            // Kill the warm pane and respawn so the removed var is
                            // absent at process level — avoids PTY echo (#137).
                            if app.warm_pane.is_some() {
                                if let Some(mut old_wp) = app.warm_pane.take() {
                                    old_wp.child.kill().ok();
                                }
                                match spawn_warm_pane(&*pty_system, &mut app) {
                                    Ok(new_wp) => {
                                        app.warm_pane = Some(new_wp);
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "psmux: warm pane respawn (UnsetEnv) failed: {e}"
                                        );
                                    }
                                }
                            }
                        }
                        CtrlReq::ShowEnvironment(resp) => {
                            let mut output = String::new();
                            // Show psmux/tmux-specific environment vars
                            for (key, value) in &app.environment {
                                output.push_str(&format!("{}={}\n", key, value));
                            }
                            // Also show inherited PSMUX_/TMUX_ vars from process env
                            for (key, value) in env::vars() {
                                if (key.starts_with("PSMUX") || key.starts_with("TMUX"))
                                    && !app.environment.contains_key(&key)
                                {
                                    output.push_str(&format!("{}={}\n", key, value));
                                }
                            }
                            let _ = resp.send(output);
                        }
                        CtrlReq::SetHook(hook, cmd) => {
                            // Replace (not append) to match tmux semantics -- prevents
                            // duplicate hooks on config reload (issue #133).
                            app.hooks.insert(hook, vec![cmd]);
                        }
                        CtrlReq::AppendHook(hook, cmd) => {
                            // -a/-ga: append to existing hook list so multiple
                            // plugins can register separate handlers (tmux semantics).
                            app.hooks.entry(hook).or_default().push(cmd);
                        }
                        CtrlReq::ShowHooks(resp) => {
                            let mut output = String::new();
                            for (name, commands) in &app.hooks {
                                for cmd in commands {
                                    output.push_str(&format!("{} -> {}\n", name, cmd));
                                }
                            }
                            if output.is_empty() {
                                output.push_str("(no hooks)\n");
                            }
                            let _ = resp.send(output);
                        }
                        CtrlReq::RemoveHook(hook) => {
                            app.hooks.remove(&hook);
                        }
                        CtrlReq::KillServer => {
                            // Remove port/key/version/pipe files FIRST so clients see the
                            // session as gone immediately, then kill processes.
                            let home = env::var("USERPROFILE")
                                .or_else(|_| env::var("HOME"))
                                .unwrap_or_default();
                            let regpath =
                                format!("{}\\.psmux\\{}.port", home, app.port_file_base());
                            let keypath = format!("{}\\.psmux\\{}.key", home, app.port_file_base());
                            let verpath =
                                format!("{}\\.psmux\\{}.version", home, app.port_file_base());
                            let pipepath =
                                format!("{}\\.psmux\\{}.pipe", home, app.port_file_base());
                            let _ = std::fs::remove_file(&regpath);
                            let _ = std::fs::remove_file(&keypath);
                            let _ = std::fs::remove_file(&verpath);
                            let _ = std::fs::remove_file(&pipepath);
                            crate::types::shutdown_persistent_streams();
                            // Kill all child processes using a single process snapshot
                            tree::kill_all_children_batch(&mut app.windows);
                            // Kill warm pane's child (process::exit skips Drop)
                            if let Some(mut wp) = app.warm_pane.take() {
                                wp.child.kill().ok();
                            }
                            // TerminateProcess is synchronous on Windows — processes
                            // are already dead.  Minimal delay for OS handle cleanup.
                            std::thread::sleep(std::time::Duration::from_millis(10));
                            std::process::exit(0);
                        }
                        CtrlReq::WaitFor(channel, op) => match op {
                            WaitForOp::Lock => {
                                let entry = app.wait_channels.entry(channel).or_insert_with(|| {
                                    WaitChannel {
                                        locked: false,
                                        waiters: Vec::new(),
                                    }
                                });
                                entry.locked = true;
                            }
                            WaitForOp::Unlock => {
                                if let Some(ch) = app.wait_channels.get_mut(&channel) {
                                    ch.locked = false;
                                    for waiter in ch.waiters.drain(..) {
                                        let _ = waiter.send(());
                                    }
                                }
                            }
                            WaitForOp::Signal => {
                                if let Some(ch) = app.wait_channels.get_mut(&channel) {
                                    for waiter in ch.waiters.drain(..) {
                                        let _ = waiter.send(());
                                    }
                                }
                            }
                            WaitForOp::Wait => {
                                app.wait_channels
                                    .entry(channel)
                                    .or_insert_with(|| WaitChannel {
                                        locked: false,
                                        waiters: Vec::new(),
                                    });
                            }
                        },
                        CtrlReq::DisplayMenu(menu_def, x, y) => {
                            let menu = parse_menu_definition(&menu_def, x, y);
                            if !menu.items.is_empty() {
                                app.mode = Mode::MenuMode { menu };
                                state_dirty = true;
                            }
                        }
                        CtrlReq::DisplayMenuDirect(menu) => {
                            if !menu.items.is_empty() {
                                app.mode = Mode::MenuMode { menu };
                                state_dirty = true;
                            }
                        }
                        CtrlReq::DisplayPopup(
                            command,
                            width_spec,
                            height_spec,
                            close_on_exit,
                            start_dir,
                        ) => {
                            // Resolve percentage dimensions against terminal area (#154)
                            let term_w = app.last_window_area.width;
                            let term_h = app.last_window_area.height;
                            let width = parse_popup_dim(&width_spec, term_w, 80);
                            let height = parse_popup_dim(&height_spec, term_h, 24);
                            // Expand format variables in start_dir (e.g. #{pane_current_path})
                            let start_dir = start_dir
                                .map(|d| expand_format(&d, &app))
                                .filter(|d| !d.is_empty());
                            let saved_dir = if start_dir.is_some() {
                                env::current_dir().ok()
                            } else {
                                None
                            };
                            if let Some(dir) = &start_dir {
                                let _ = env::set_current_dir(dir);
                            }
                            if !command.is_empty() {
                                // Spawn popup as a real Pane via the popup module
                                let inner_h = height.saturating_sub(2);
                                let inner_w = width.saturating_sub(2);
                                let pane_result = crate::popup::create_popup_pane(
                                    &command,
                                    start_dir.as_deref(),
                                    inner_h,
                                    inner_w,
                                    app.next_pane_id,
                                    &app.session_name,
                                    &app.environment,
                                );
                                if let Some(prev) = saved_dir {
                                    let _ = env::set_current_dir(prev);
                                }

                                app.mode = Mode::PopupMode {
                                    command: command.clone(),
                                    output: String::new(),
                                    process: None,
                                    width,
                                    height,
                                    close_on_exit,
                                    popup_pane: pane_result.map(Box::new),
                                    scroll_offset: 0,
                                };
                                state_dirty = true;
                            } else {
                                if let Some(prev) = saved_dir {
                                    let _ = env::set_current_dir(prev);
                                }
                                app.mode = Mode::PopupMode {
                                    command: String::new(),
                                    output: "Press 'q' or Escape to close\n".to_string(),
                                    process: None,
                                    width,
                                    height,
                                    close_on_exit: true,
                                    popup_pane: None,
                                    scroll_offset: 0,
                                };
                                state_dirty = true;
                            }
                        }
                        CtrlReq::ConfirmBefore(prompt, cmd) => {
                            let prompt_text = if prompt.is_empty() {
                                format!("Confirm: {}? (y/n)", cmd)
                            } else {
                                // Don't append (y/n) if prompt already contains it
                                if prompt.contains("(y/n)") {
                                    prompt.clone()
                                } else {
                                    let base = prompt.trim_end_matches('?');
                                    format!("{}? (y/n)", base)
                                }
                            };
                            app.mode = Mode::ConfirmMode {
                                prompt: prompt_text,
                                command: cmd,
                                input: String::new(),
                            };
                            state_dirty = true;
                        }
                        CtrlReq::ResizePaneAbsolute(axis, size) => {
                            unzoom_if_zoomed(&mut app);
                            resize_pane_absolute(&mut app, &axis, size);
                        }
                        CtrlReq::ResizePanePercent(axis, pct) => {
                            unzoom_if_zoomed(&mut app);
                            // Convert percentage to absolute size based on current window dimensions
                            let area = app.last_window_area;
                            let total = if axis == "x" { area.width } else { area.height };
                            let abs_size = ((total as u32) * (pct as u32) / 100).max(1) as u16;
                            resize_pane_absolute(&mut app, &axis, abs_size);
                        }
                        CtrlReq::ShowOptionValue(resp, name) => {
                            let val = get_option_value(&app, &name);
                            let _ = resp.send(val);
                        }
                        CtrlReq::ShowWindowOptionValue(resp, name) => {
                            let val = get_window_option_value(&app, &name);
                            let _ = resp.send(val);
                        }
                        CtrlReq::ShowWindowOptions(resp) => {
                            let _ = resp.send(render_window_options(&app));
                        }
                        CtrlReq::ChooseBuffer(resp) => {
                            let mut output = String::new();
                            for (i, buf) in app.paste_buffers.iter().enumerate() {
                                let preview: String = buf.chars().take(50).collect();
                                let preview = preview.replace('\n', "\\n").replace('\r', "");
                                output.push_str(&format!(
                                    "buffer{}: {} bytes: \"{}\"\n",
                                    i,
                                    buf.len(),
                                    preview
                                ));
                            }
                            let _ = resp.send(output);
                        }
                        CtrlReq::ServerInfo(resp) => {
                            let info = format!(
                        "psmux {} (Windows)\npid: {}\nsession: {}\nwindows: {}\nuptime: {}s\nsocket: {}",
                        VERSION,
                        std::process::id(),
                        app.session_name,
                        app.windows.len(),
                        (chrono::Local::now() - app.created_at).num_seconds(),
                        {
                            let home = env::var("USERPROFILE").or_else(|_| env::var("HOME")).unwrap_or_default();
                            format!("{}\\.psmux\\{}.port", home, app.port_file_base())
                        }
                    );
                            let _ = resp.send(info);
                        }
                        CtrlReq::SendPrefix => {
                            // Send the prefix key to the active pane as if typed
                            let prefix = app.prefix_key;
                            let encoded: Vec<u8> = match prefix.0 {
                                crossterm::event::KeyCode::Char(c)
                                    if prefix
                                        .1
                                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                                {
                                    vec![(c.to_ascii_lowercase() as u8) & 0x1F]
                                }
                                crossterm::event::KeyCode::Char(c) => format!("{}", c).into_bytes(),
                                _ => vec![],
                            };
                            if !encoded.is_empty() {
                                let win = &mut app.windows[app.active_idx];
                                if let Some(p) = active_pane_mut(&mut win.root, &win.active_path) {
                                    let _ = p.writer.write_all(&encoded);
                                    let _ = p.writer.flush();
                                }
                            }
                        }
                        CtrlReq::PrevLayout => {
                            unzoom_if_zoomed(&mut app);
                            cycle_layout_reverse(&mut app);
                            state_dirty = true;
                        }
                        CtrlReq::FocusIn => {
                            app.window_focused = true;
                            state_dirty = true;
                            if app.focus_events {
                                // Forward focus-in escape sequence to all panes in active window
                                let win = &mut app.windows[app.active_idx];
                                fn send_focus_seq(node: &mut Node, seq: &[u8]) {
                                    match node {
                                        Node::Leaf(p) => {
                                            let _ = p.writer.write_all(seq);
                                            let _ = p.writer.flush();
                                        }
                                        Node::Split { children, .. } => {
                                            for c in children {
                                                send_focus_seq(c, seq);
                                            }
                                        }
                                    }
                                }
                                send_focus_seq(&mut win.root, b"\x1b[I");
                            }
                            hook_event = Some("pane-focus-in");
                        }
                        CtrlReq::FocusOut => {
                            app.window_focused = false;
                            state_dirty = true;
                            if app.focus_events {
                                let win = &mut app.windows[app.active_idx];
                                fn send_focus_seq(node: &mut Node, seq: &[u8]) {
                                    match node {
                                        Node::Leaf(p) => {
                                            let _ = p.writer.write_all(seq);
                                            let _ = p.writer.flush();
                                        }
                                        Node::Split { children, .. } => {
                                            for c in children {
                                                send_focus_seq(c, seq);
                                            }
                                        }
                                    }
                                }
                                send_focus_seq(&mut win.root, b"\x1b[O");
                            }
                            hook_event = Some("pane-focus-out");
                        }
                        CtrlReq::CommandPrompt(initial) => {
                            app.mode = Mode::CommandPrompt {
                                input: initial.clone(),
                                cursor: initial.len(),
                            };
                            state_dirty = true;
                        }
                        CtrlReq::ShowMessages(resp) => {
                            // Return message log (tmux stores recent log messages)
                            let _ = resp.send(String::new());
                        }
                        CtrlReq::ResizeWindow(_dim, _size) => {
                            // On Windows, window size is controlled by the terminal emulator;
                            // resize-window is a no-op since we adapt to the terminal size.
                        }
                        CtrlReq::RespawnWindow => {
                            // Kill all panes in the active window and respawn
                            respawn_active_pane(&mut app, Some(&*pty_system), true)?;
                            state_dirty = true;
                        }
                        CtrlReq::PopupInput(data) => {
                            if let Mode::PopupMode {
                                ref mut popup_pane, ..
                            } = app.mode
                            {
                                if let Some(ref mut pty) = popup_pane {
                                    // If child has exited, 'q' closes the popup
                                    let child_exited = matches!(pty.child.try_wait(), Ok(Some(_)));
                                    if child_exited && data == b"q" {
                                        app.mode = Mode::Passthrough;
                                    } else if !child_exited {
                                        let _ = pty.writer.write_all(&data);
                                        let _ = pty.writer.flush();
                                    }
                                } else {
                                    // No PTY means static popup — 'q' closes it
                                    if data == b"q" {
                                        app.mode = Mode::Passthrough;
                                    }
                                }
                            }
                            state_dirty = true;
                        }
                        CtrlReq::OverlayClose => match app.mode {
                            Mode::PopupMode { .. }
                            | Mode::MenuMode { .. }
                            | Mode::ConfirmMode { .. }
                            | Mode::PaneChooser { .. }
                            | Mode::ClockMode
                            | Mode::HintsMode(_) => {
                                app.mode = Mode::Passthrough;
                                state_dirty = true;
                            }
                            _ => {}
                        },
                        CtrlReq::ConfirmRespond(yes) => {
                            if let Mode::ConfirmMode { ref command, .. } = app.mode {
                                let cmd = command.clone();
                                app.mode = Mode::Passthrough;
                                if yes {
                                    let _ = execute_command_string(&mut app, &cmd);
                                }
                                state_dirty = true;
                            }
                        }
                        CtrlReq::MenuSelect(idx) => {
                            if let Mode::MenuMode { ref menu } = app.mode {
                                if let Some(item) = menu.items.get(idx) {
                                    if !item.is_separator && !item.command.is_empty() {
                                        let cmd = item.command.clone();
                                        app.mode = Mode::Passthrough;
                                        let _ = execute_command_string(&mut app, &cmd);
                                        state_dirty = true;
                                    }
                                }
                            }
                        }
                        CtrlReq::MenuNavigate(delta) => {
                            if let Mode::MenuMode { ref mut menu } = app.mode {
                                let len = menu.items.len();
                                if len > 0 {
                                    if delta > 0 {
                                        // Move down, skipping separators
                                        let mut next = (menu.selected + 1) % len;
                                        let start = next;
                                        while menu.items[next].is_separator {
                                            next = (next + 1) % len;
                                            if next == start {
                                                break;
                                            }
                                        }
                                        menu.selected = next;
                                    } else {
                                        // Move up, skipping separators
                                        let mut next = if menu.selected == 0 {
                                            len - 1
                                        } else {
                                            menu.selected - 1
                                        };
                                        let start = next;
                                        while menu.items[next].is_separator {
                                            next = if next == 0 { len - 1 } else { next - 1 };
                                            if next == start {
                                                break;
                                            }
                                        }
                                        menu.selected = next;
                                    }
                                    state_dirty = true;
                                }
                            }
                        }

                        CtrlReq::HintsInput(ch) => {
                            if let Mode::HintsMode(ref mut state) = app.mode {
                                state.input.push(ch);
                                if let Some(m) =
                                    crate::hints::find_match(&state.matches, &state.input)
                                {
                                    let text = m.text.clone();
                                    crate::copy_mode::copy_to_system_clipboard(&text);
                                    if app.set_clipboard != "off" {
                                        app.clipboard_osc52 = Some(text.clone());
                                    }
                                    app.status_message = Some((
                                        format!("Copied: {}", text),
                                        std::time::Instant::now(),
                                    ));
                                    app.mode = Mode::Passthrough;
                                } else if !crate::hints::has_prefix(&state.matches, &state.input) {
                                    state.input.clear();
                                }
                                state_dirty = true;
                            }
                        }

                        // ── Backend JSON-RPC handlers (CustomPaneBackend) ──
                        CtrlReq::BackendInitialize { resp } => {
                            // Return the active pane's context ID as "%{id}".
                            let pane_id = get_active_pane_id(
                                &app.windows[app.active_idx].root,
                                &app.windows[app.active_idx].active_path,
                            )
                            .unwrap_or(0);
                            let _ = resp.send(format!("%{}", pane_id));
                        }
                        CtrlReq::BackendSpawnAgent {
                            command,
                            cwd,
                            env: extra_env,
                            metadata,
                            split_direction,
                            shell,
                            mode,
                            window_name,
                            resp,
                        } => {
                            // Build command string: join argv into a single
                            // shell command for split_active_with_command.
                            // Empty command spawns a default shell pane (warm pane path).
                            let cmd_str = command.join(" ");
                            let cmd_str = if cmd_str.is_empty() {
                                None
                            } else {
                                Some(cmd_str)
                            };
                            let start_dir = cwd
                                .map(|d| expand_format(&d, &app))
                                .filter(|d| !d.is_empty());
                            let saved_dir = if start_dir.is_some() {
                                env::current_dir().ok()
                            } else {
                                None
                            };
                            if let Some(dir) = &start_dir {
                                env::set_current_dir(dir).ok();
                            }
                            // Temporarily stash warm pane when custom dir or shell override is given
                            let stashed_warm = if start_dir.is_some() || shell.is_some() {
                                app.warm_pane.take()
                            } else {
                                None
                            };
                            // Set extra env vars in the process environment
                            // before spawning (they'll be inherited via ConPTY).
                            let mut saved_envs: Vec<(String, Option<String>)> = Vec::new();
                            if let Some(ref vars) = extra_env {
                                for (k, v) in vars {
                                    saved_envs.push((k.clone(), env::var(k).ok()));
                                    crate::util::set_env(k, v);
                                }
                            }
                            let use_window = mode.as_deref() == Some("window");
                            let spawn_result = if use_window {
                                // Window mode: create a new window (always detached)
                                let prev_idx = app.active_idx;
                                let r = create_window(
                                    &*pty_system,
                                    &mut app,
                                    cmd_str.as_deref(),
                                    start_dir.as_deref(),
                                    shell.as_deref(),
                                );
                                // Set window name if provided
                                if r.is_ok() {
                                    if let Some(n) = window_name {
                                        if let Some(w) = app.windows.last_mut() {
                                            w.name = n;
                                            w.manual_rename = true;
                                        }
                                    }
                                    // Always detached: restore focus to previous window
                                    app.active_idx = prev_idx;
                                }
                                r
                            } else {
                                // Split mode (default): split the active pane
                                split_active_with_command(
                                    &mut app,
                                    split_direction.unwrap_or(LayoutKind::Vertical),
                                    cmd_str.as_deref(),
                                    Some(&*pty_system),
                                    start_dir.as_deref(),
                                    shell.as_deref(),
                                )
                            };
                            // Restore stashed env vars
                            for (k, prev) in saved_envs {
                                if let Some(v) = prev {
                                    crate::util::set_env(&k, v);
                                } else {
                                    crate::util::remove_env(&k);
                                }
                            }
                            if let Some(wp) = stashed_warm {
                                app.warm_pane = Some(wp);
                            }
                            match spawn_result {
                                Ok(()) => {
                                    let new_pane_id = if use_window {
                                        // New window: pane is the root of the last window
                                        app.windows
                                            .last()
                                            .and_then(|w| {
                                                crate::tree::active_pane(&w.root, &w.active_path)
                                                    .map(|p| p.id)
                                            })
                                            .unwrap_or(0)
                                    } else {
                                        get_active_pane_id(
                                            &app.windows[app.active_idx].root,
                                            &app.windows[app.active_idx].active_path,
                                        )
                                        .unwrap_or(0)
                                    };
                                    // Apply metadata to the new pane
                                    if let Some(meta) = metadata {
                                        let target_idx = if use_window {
                                            app.windows.len().saturating_sub(1)
                                        } else {
                                            app.active_idx
                                        };
                                        let win = &mut app.windows[target_idx];
                                        if let Some(p) =
                                            active_pane_mut(&mut win.root, &win.active_path)
                                        {
                                            meta.apply_to(&mut p.metadata);
                                        }
                                    }
                                    resize_all_panes(&mut app);
                                    meta_dirty = true;
                                    if use_window {
                                        hook_event = Some("after-new-window");
                                        crate::resurrection::save_snapshot(&app);
                                    }
                                    // Replenish warm pane
                                    if app.warm_pane.is_none() {
                                        if let Ok(wp) = spawn_warm_pane(&*pty_system, &mut app) {
                                            app.warm_pane = Some(wp);
                                        }
                                    }
                                    let _ = resp.send(format!("%{}", new_pane_id));
                                }
                                Err(e) => {
                                    let _ = resp.send(format!("ERROR:{}", e));
                                }
                            }
                            if let Some(prev) = saved_dir {
                                env::set_current_dir(prev).ok();
                            }
                        }
                        CtrlReq::BackendCapturePane {
                            pane_id,
                            lines,
                            clean,
                            resp,
                        } => {
                            // Parse "%N" format to get numeric pane ID.
                            let id = pane_id
                                .strip_prefix('%')
                                .and_then(|s| s.parse::<usize>().ok());
                            let mut captured = String::new();
                            let mut found = false;
                            if let Some(pid) = id {
                                // Find the pane across all windows
                                for win in &app.windows {
                                    if let Some(path) = crate::tree::find_path_by_id(&win.root, pid)
                                    {
                                        found = true;
                                        if let Some(p) = crate::tree::active_pane(&win.root, &path)
                                        {
                                            if let Ok(parser) = p.term.lock() {
                                                let screen = parser.screen();
                                                for row in 0..p.last_rows {
                                                    let mut line = String::new();
                                                    for col in 0..p.last_cols {
                                                        if let Some(cell) = screen.cell(row, col) {
                                                            line.push_str(cell.contents());
                                                        } else {
                                                            line.push(' ');
                                                        }
                                                    }
                                                    captured.push_str(line.trim_end());
                                                    captured.push('\n');
                                                }
                                            }
                                        }
                                        break;
                                    }
                                }
                            }
                            if !found {
                                let _ = resp.send("__PANE_NOT_FOUND__".to_string());
                            } else {
                                // Clean mode: strip trailing blank lines
                                if clean {
                                    while captured.ends_with("\n\n") {
                                        captured.pop();
                                    }
                                }
                                // Line limiting: return only the last N lines
                                if let Some(max_lines) = lines {
                                    let max = max_lines as usize;
                                    let all_lines: Vec<&str> = captured.lines().collect();
                                    if all_lines.len() > max {
                                        captured = all_lines[all_lines.len() - max..].join("\n");
                                        captured.push('\n');
                                    }
                                }
                                let _ = resp.send(captured);
                            }
                        }
                        CtrlReq::BackendListPanes { resp } => {
                            // Build a JSON array of all panes across all windows.
                            let mut contexts: Vec<crate::backend::protocol::ContextInfo> =
                                Vec::new();
                            for win in &app.windows {
                                fn collect_backend_panes(
                                    node: &Node,
                                    out: &mut Vec<crate::backend::protocol::ContextInfo>,
                                ) {
                                    match node {
                                        Node::Leaf(p) => {
                                            let meta = crate::backend::protocol::AgentMetadata::from_metadata_map(&p.metadata);
                                            out.push(crate::backend::protocol::ContextInfo {
                                                context_id: format!("%{}", p.id),
                                                alive: !p.dead,
                                                cwd: p
                                                    .spawn_cwd
                                                    .as_ref()
                                                    .map(|c| c.to_string_lossy().into_owned()),
                                                title: if p.title.is_empty() {
                                                    None
                                                } else {
                                                    Some(p.title.clone())
                                                },
                                                shell_name: p.shell_name.clone(),
                                                metadata: meta,
                                            });
                                        }
                                        Node::Split { children, .. } => {
                                            for c in children {
                                                collect_backend_panes(c, out);
                                            }
                                        }
                                    }
                                }
                                collect_backend_panes(&win.root, &mut contexts);
                            }
                            let result = crate::backend::protocol::ListResult { contexts };
                            let json = serde_json::to_string(&result)
                                .unwrap_or_else(|_| r#"{"contexts":[]}"#.to_string());
                            let _ = resp.send(json);
                        }
                        CtrlReq::BackendKillPane {
                            pane_id,
                            grace_ms,
                            resp,
                        } => {
                            let id = pane_id
                                .strip_prefix('%')
                                .and_then(|s| s.parse::<usize>().ok());
                            if let Some(pid) = id {
                                if let Some(grace) = grace_ms {
                                    // Non-blocking graceful kill: send CTRL_BREAK, then spawn a
                                    // background thread to force-kill after the grace period.
                                    if let Some(raw_pid) = get_pane_process_id(&app, pid) {
                                        // SAFETY: GenerateConsoleCtrlEvent sends CTRL_BREAK to
                                        // the process group. raw_pid is a valid process ID.
                                        unsafe {
                                            crate::platform::mouse_inject::generate_ctrl_break(
                                                raw_pid,
                                            );
                                        }
                                    }
                                    // Spawn background thread for delayed force-kill.
                                    if let Some(ref ctrl_tx) = app.control_tx {
                                        let tx_clone = ctrl_tx.clone();
                                        let pane_id_str = format!("%{}", pid);
                                        std::thread::spawn(move || {
                                            std::thread::sleep(std::time::Duration::from_millis(
                                                grace,
                                            ));
                                            let (resp_tx, _) = mpsc::channel();
                                            let _ = tx_clone.send(CtrlReq::BackendKillPane {
                                                pane_id: pane_id_str,
                                                grace_ms: None,
                                                resp: resp_tx,
                                            });
                                        });
                                    }
                                } else {
                                    // Immediate kill (no grace period)
                                    unzoom_if_zoomed(&mut app);
                                    let _ = kill_pane_by_id(&mut app, pid);
                                    resize_all_panes(&mut app);
                                    meta_dirty = true;
                                }
                            }
                            let _ = resp.send(());
                        }
                        CtrlReq::BackendKillAll { role, resp } => {
                            let mut killed_ids: Vec<String> = Vec::new();
                            let mut pane_ids_to_kill: Vec<usize> = Vec::new();
                            for win in &app.windows {
                                fn collect_agent_panes(
                                    node: &Node,
                                    role_filter: &Option<String>,
                                    out: &mut Vec<usize>,
                                ) {
                                    match node {
                                        Node::Leaf(p) => {
                                            if p.metadata.contains_key("@agent") {
                                                if let Some(ref role) = role_filter {
                                                    if p.metadata
                                                        .get("@role")
                                                        .map(|r| r == role)
                                                        .unwrap_or(false)
                                                    {
                                                        out.push(p.id);
                                                    }
                                                } else {
                                                    out.push(p.id);
                                                }
                                            }
                                        }
                                        Node::Split { children, .. } => {
                                            for c in children {
                                                collect_agent_panes(c, role_filter, out);
                                            }
                                        }
                                    }
                                }
                                collect_agent_panes(&win.root, &role, &mut pane_ids_to_kill);
                            }
                            unzoom_if_zoomed(&mut app);
                            for pid in pane_ids_to_kill {
                                if kill_pane_by_id(&mut app, pid).is_ok() {
                                    killed_ids.push(format!("%{}", pid));
                                }
                            }
                            if !killed_ids.is_empty() {
                                resize_all_panes(&mut app);
                                meta_dirty = true;
                            }
                            let _ = resp.send(killed_ids);
                        }
                        CtrlReq::BackendSetMetadata {
                            pane_id,
                            metadata,
                            resp,
                        } => {
                            let id = pane_id
                                .strip_prefix('%')
                                .and_then(|s| s.parse::<usize>().ok());
                            let mut found = false;
                            if let Some(pid) = id {
                                for win in &mut app.windows {
                                    if let Some(path) = crate::tree::find_path_by_id(&win.root, pid)
                                    {
                                        if let Some(p) =
                                            crate::tree::active_pane_mut(&mut win.root, &path)
                                        {
                                            metadata.apply_to(&mut p.metadata);
                                            found = true;
                                        }
                                        break;
                                    }
                                }
                                if found {
                                    meta_dirty = true;
                                }
                            }
                            let _ = resp.send(found);
                        }
                        CtrlReq::BackendSendText {
                            pane_id,
                            text,
                            resp,
                        } => {
                            let id = pane_id
                                .strip_prefix('%')
                                .and_then(|s| s.parse::<usize>().ok());
                            let mut sent = false;
                            if let Some(pid) = id {
                                // Find the pane and write text to its PTY writer.
                                for win in &mut app.windows {
                                    if let Some(path) = crate::tree::find_path_by_id(&win.root, pid)
                                    {
                                        if let Some(p) =
                                            crate::tree::active_pane_mut(&mut win.root, &path)
                                        {
                                            let _ = p.writer.write_all(text.as_bytes());
                                            let _ = p.writer.flush();
                                            sent = true;
                                        }
                                        break;
                                    }
                                }
                            }
                            if let Some(tx) = resp {
                                let _ = tx.send(sent);
                            }
                        }
                        CtrlReq::BackendRunShell { context_id, resp } => {
                            let cwd = if let Some(ref cid) = context_id {
                                let pane_id_str = cid.trim_start_matches('%');
                                if let Ok(pid) = pane_id_str.parse::<usize>() {
                                    let mut found_cwd = None;
                                    for win in app.windows.iter() {
                                        if let Some(path) =
                                            crate::tree::find_path_by_id(&win.root, pid)
                                        {
                                            if let Some(p) =
                                                crate::tree::active_pane(&win.root, &path)
                                            {
                                                found_cwd = p.spawn_cwd.clone();
                                            }
                                            break;
                                        }
                                    }
                                    found_cwd
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            let _ = resp.send(cwd);
                        }
                        CtrlReq::SendKeysHex(hex_str) => {
                            // send-keys -H: parse hex bytes and write raw to active pane PTY
                            let bytes: Vec<u8> = hex_str
                                .split_whitespace()
                                .filter_map(|tok| u8::from_str_radix(tok, 16).ok())
                                .collect();
                            if !bytes.is_empty() {
                                let win = &mut app.windows[app.active_idx];
                                if let Some(p) = active_pane_mut(&mut win.root, &win.active_path) {
                                    let _ = p.writer.write_all(&bytes);
                                    let _ = p.writer.flush();
                                }
                            }
                        }
                        CtrlReq::Exec {
                            command,
                            shell,
                            pane_id,
                            resp,
                        } => {
                            // Gather pane context on the server thread (fast),
                            // then spawn the actual command on a background thread
                            // to avoid blocking the server event loop.
                            let (mut exec_cwd, pane_pid) = if let Some(pid) = pane_id {
                                let mut found_cwd = None;
                                let mut found_pid = None;
                                for win in &app.windows {
                                    if let Some(path) = crate::tree::find_path_by_id(&win.root, pid)
                                    {
                                        if let Some(p) = crate::tree::active_pane(&win.root, &path)
                                        {
                                            found_cwd = p.spawn_cwd.clone();
                                            found_pid = p.child_pid;
                                        }
                                        break;
                                    }
                                }
                                (
                                    found_cwd.or_else(|| std::env::current_dir().ok()),
                                    found_pid,
                                )
                            } else {
                                let win = &app.windows[app.active_idx];
                                let cwd = active_pane(&win.root, &win.active_path)
                                    .and_then(|p| p.spawn_cwd.clone())
                                    .or_else(|| std::env::current_dir().ok());
                                let pid = active_pane(&win.root, &win.active_path)
                                    .and_then(|p| p.child_pid);
                                (cwd, pid)
                            };

                            // Try to get the pane's actual cwd from its process
                            if let Some(pid) = pane_pid {
                                if let Some(real_cwd) =
                                    crate::platform::process_info::get_foreground_cwd(pid)
                                {
                                    exec_cwd = Some(std::path::PathBuf::from(real_cwd));
                                }
                            }

                            let shell_program = shell
                                .or_else(|| {
                                    if app.default_shell.is_empty() {
                                        None
                                    } else {
                                        Some(app.default_shell.clone())
                                    }
                                })
                                .unwrap_or_else(|| "pwsh".to_string());

                            let env_snapshot: Vec<(String, String)> = app
                                .environment
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();

                            // Run on a background thread so the server loop stays responsive
                            std::thread::spawn(move || {
                                let lower = shell_program.to_lowercase();
                                let mut cmd = std::process::Command::new(&shell_program);

                                if lower.contains("pwsh") || lower.contains("powershell") {
                                    cmd.args(["-NoProfile", "-NoLogo", "-Command", &command]);
                                } else if lower.contains("bash")
                                    || lower.contains("sh")
                                    || lower.contains("zsh")
                                {
                                    cmd.args(["-c", &command]);
                                } else if lower.contains("cmd") {
                                    cmd.args(["/C", &command]);
                                } else {
                                    cmd.args(["-c", &command]);
                                }

                                if let Some(ref cwd) = exec_cwd {
                                    cmd.current_dir(cwd);
                                }
                                for (k, v) in &env_snapshot {
                                    cmd.env(k, v);
                                }

                                let start = std::time::Instant::now();
                                let result = match cmd.output() {
                                    Ok(output) => {
                                        let elapsed_ms = start.elapsed().as_millis() as u64;
                                        let stdout =
                                            String::from_utf8_lossy(&output.stdout).into_owned();
                                        let stderr =
                                            String::from_utf8_lossy(&output.stderr).into_owned();
                                        let exit_code = output.status.code().unwrap_or(-1);
                                        format!(
                                            "{{\"exit_code\":{},\"stdout\":{},\"stderr\":{},\"elapsed_ms\":{}}}",
                                            exit_code,
                                            serde_json::to_string(&stdout)
                                                .unwrap_or_else(|_| "\"\"".into()),
                                            serde_json::to_string(&stderr)
                                                .unwrap_or_else(|_| "\"\"".into()),
                                            elapsed_ms,
                                        )
                                    }
                                    Err(e) => {
                                        let elapsed_ms = start.elapsed().as_millis() as u64;
                                        format!(
                                            "{{\"exit_code\":-1,\"stdout\":\"\",\"stderr\":{},\"elapsed_ms\":{}}}",
                                            serde_json::to_string(&e.to_string())
                                                .unwrap_or_else(|_| "\"exec failed\"".into()),
                                            elapsed_ms,
                                        )
                                    }
                                };
                                let _ = resp.send(result);
                            });
                        }
                        CtrlReq::ShowTextPopup(title, content) => {
                            let lines: Vec<&str> = content.lines().collect();
                            let width = lines.iter().map(|l| l.len()).max().unwrap_or(40).max(20)
                                as u16
                                + 4;
                            let height = (lines.len() as u16 + 2).clamp(5, 40);
                            app.mode = Mode::PopupMode {
                                command: title,
                                output: content,
                                process: None,
                                width: width.min(120),
                                height,
                                close_on_exit: false,
                                popup_pane: None,
                                scroll_offset: 0,
                            };
                            state_dirty = true;
                        }
                    }
                    // Log any active_idx change for debugging window-switch issues
                    if app.active_idx != _prev_active_idx && crate::debug_log::server_log_enabled()
                    {
                        crate::debug_log::server_log(
                            "switch",
                            &format!(
                                "active_idx changed {} -> {} by req={} hook={:?}",
                                _prev_active_idx, app.active_idx, _req_tag, hook_event
                            ),
                        );
                    }
                    // Fire any hooks registered for the event that just occurred
                    if let Some(event) = hook_event {
                        let _pre_hook_idx = app.active_idx;
                        crate::commands::fire_hooks(&mut app, event);
                        // Check if the hook itself changed active_idx
                        if app.active_idx != _pre_hook_idx && crate::debug_log::server_log_enabled()
                        {
                            crate::debug_log::server_log(
                                "switch",
                                &format!(
                                    "active_idx changed {} -> {} by HOOK event={}",
                                    _pre_hook_idx, app.active_idx, event
                                ),
                            );
                        }
                    }
                    // Restore temporary -t focus after non-temp command completes.
                    // Use pane ID (not path) because kill-pane restructures the
                    // tree and invalidates saved paths (#71).
                    if !is_temp_focus {
                        if let Some((restore_idx, restore_pane_id)) = temp_focus_restore.take() {
                            if restore_idx < app.windows.len() {
                                app.active_idx = restore_idx;
                                let win = &mut app.windows[restore_idx];
                                if let Some(path) =
                                    crate::tree::find_path_by_id(&win.root, restore_pane_id)
                                {
                                    win.active_path = path;
                                }
                                // If the pane was killed, keep whatever active_path
                                // kill_pane_at_path already set (MRU target).
                            }
                        }
                    }
                    if mutates_state {
                        state_dirty = true;
                    }
                }
                // No trailing cleanup: temp_focus_restore persists across
                // batch boundaries so the actual command that follows in a
                // later batch can still benefit from the temp focus (and
                // will restore when it processes as a non-temp-focus req).
            }
        }
        // Drain async run-shell results (non-blocking).
        if let Some(rx) = app.run_shell_rx.as_ref() {
            while let Ok((title, text)) = rx.try_recv() {
                if !text.is_empty() {
                    let lines: Vec<&str> = text.lines().collect();
                    let width =
                        lines.iter().map(|l| l.len()).max().unwrap_or(40).max(20) as u16 + 4;
                    let height = (lines.len() as u16 + 2).max(5);
                    app.mode = Mode::PopupMode {
                        command: title,
                        output: text,
                        process: None,
                        width: width.min(120),
                        height,
                        close_on_exit: false,
                        popup_pane: None,
                        scroll_offset: 0,
                    };
                    state_dirty = true;
                }
            }
        }
        // ── Server-push: proactively send frames to attached clients ──
        // Instead of waiting for clients to poll dump-state, serialize
        // and push whenever state changed (PTY output, new window, key
        // echo, etc.).  This gives event-driven rendering like wezterm:
        // frames arrive within 1-5ms of ConPTY output instead of waiting
        // for the next client poll cycle (up to 50ms).
        if (state_dirty || meta_dirty) && crate::types::has_frame_receivers() {
            // Rebuild metadata cache if structural changes happened.
            if meta_dirty {
                cached_windows_json = list_windows_json_with_tabs(&app)?;
                cached_tree_json = list_tree_json(&app)?;
                cached_prefix_str = format_key_binding(&app.prefix_key);
                cached_prefix2_str = app
                    .prefix2_key
                    .as_ref()
                    .map(format_key_binding)
                    .unwrap_or_default();
                cached_base_index = app.window_base_index;
                cached_pred_dim = app.prediction_dimming;
                cached_status_style = app.status_style.clone();
                cached_bindings_json = serialize_bindings_json(&app);
                meta_dirty = false;
            }
            let layout_json = dump_layout_json_fast(&mut app)?;
            combined_buf.clear();
            let ss_escaped = json_escape_string(&cached_status_style);
            let sl_expanded = json_escape_string(&expand_format(&app.status_left, &app));
            let sr_expanded = json_escape_string(&expand_format(&app.status_right, &app));
            let pbs_escaped = json_escape_string(&app.pane_border_style);
            let pabs_escaped = json_escape_string(&app.pane_active_border_style);
            let pbs_status_escaped = json_escape_string(&app.pane_border_status);
            let pbf_escaped = json_escape_string(&app.pane_border_format);
            let sus_escaped = json_escape_string(&app.status_unfocused_style);
            let wsf_escaped = json_escape_string(&app.window_status_format);
            let wscf_escaped = json_escape_string(&app.window_status_current_format);
            let wss_escaped = json_escape_string(&app.window_status_separator);
            let ws_style_escaped = json_escape_string(&app.window_status_style);
            let wsc_style_escaped = json_escape_string(&app.window_status_current_style);
            let mode_style_escaped = json_escape_string(&app.mode_style);
            let status_position_escaped = json_escape_string(&app.status_position);
            let status_justify_escaped = json_escape_string(&app.status_justify);
            let status_format_json = {
                let mut sf = String::from("[");
                for (i, fmt_str) in app.status_format.iter().enumerate() {
                    if i > 0 {
                        sf.push(',');
                    }
                    sf.push('"');
                    sf.push_str(&json_escape_string(&expand_format(fmt_str, &app)));
                    sf.push('"');
                }
                sf.push(']');
                sf
            };
            let cursor_style_code = crate::rendering::configured_cursor_code();
            let _ = std::fmt::Write::write_fmt(&mut combined_buf, format_args!(
                "{{\"layout\":{},\"windows\":{},\"prefix\":\"{}\",\"prefix2\":\"{}\",\"tree\":{},\"base_index\":{},\"prediction_dimming\":{},\"status_style\":\"{}\",\"status_left\":\"{}\",\"status_right\":\"{}\",\"pane_border_style\":\"{}\",\"pane_active_border_style\":\"{}\",\"wsf\":\"{}\",\"wscf\":\"{}\",\"wss\":\"{}\",\"ws_style\":\"{}\",\"wsc_style\":\"{}\",\"clock_mode\":{},\"bindings\":{},\"defaults_suppressed\":{},\"status_left_length\":{},\"status_right_length\":{},\"status_lines\":{},\"status_format\":{},\"mode_style\":\"{}\",\"status_position\":\"{}\",\"status_justify\":\"{}\",\"cursor_style_code\":{},\"status_visible\":{},\"repeat_time\":{},\"zoomed\":{},\"pane_border_status\":\"{}\",\"pane_border_format\":\"{}\",\"status_unfocused_style\":\"{}\",\"sync_input\":{}}}",
                layout_json, cached_windows_json, cached_prefix_str, cached_prefix2_str, cached_tree_json, cached_base_index, cached_pred_dim, ss_escaped, sl_expanded, sr_expanded, pbs_escaped, pabs_escaped, wsf_escaped, wscf_escaped, wss_escaped, ws_style_escaped, wsc_style_escaped,
                matches!(app.mode, Mode::ClockMode), cached_bindings_json, app.defaults_suppressed,
                app.status_left_length, app.status_right_length, app.status_lines, status_format_json,
                mode_style_escaped, status_position_escaped, status_justify_escaped,
                cursor_style_code, app.status_visible, app.repeat_time_ms,
                app.windows.get(app.active_idx).is_some_and(|w| w.zoom_saved.is_some()),
                pbs_status_escaped, pbf_escaped, sus_escaped, app.sync_input,
            ));
            // Inject overlay state (popup, menu, confirm, display_panes)
            {
                let overlay_json = serialize_overlay_json(&app);
                if !overlay_json.is_empty() && combined_buf.ends_with('}') {
                    combined_buf.pop();
                    combined_buf.push_str(&overlay_json);
                    combined_buf.push('}');
                }
            }
            // Inject clipboard data if pending
            if let Some(clip_text) = app.clipboard_osc52.take() {
                let clip_b64 = base64_encode(&clip_text);
                if combined_buf.ends_with('}') {
                    combined_buf.pop();
                    combined_buf.push_str(",\"clipboard_osc52\":\"");
                    combined_buf.push_str(&clip_b64);
                    combined_buf.push_str("\"}");
                }
            }
            cached_dump_state.clear();
            cached_dump_state.push_str(&combined_buf);
            cached_data_version = combined_data_version(&app);
            state_dirty = false;
            if crate::debug_log::memory_log_enabled() {
                let in_copy = matches!(app.mode, Mode::CopyMode | Mode::CopySearch { .. });
                if in_copy {
                    crate::debug_log::memory_log(
                        "serverpush",
                        &format!(
                            "server-push frame in COPY MODE: size={} (cap={})",
                            crate::debug_log::format_bytes(combined_buf.len() as u64),
                            crate::debug_log::format_bytes(combined_buf.capacity() as u64),
                        ),
                    );
                }
            }
            crate::types::push_frame(&combined_buf);
        }
        // ── Status-interval timer: fire hooks periodically ──
        if app.status_interval > 0 {
            let elapsed = app.last_status_interval_fire.elapsed().as_secs();
            if elapsed >= app.status_interval {
                app.last_status_interval_fire = std::time::Instant::now();
                let _pre_status_idx = app.active_idx;
                {
                    let cmds: Vec<String> = app
                        .hooks
                        .get("status-interval")
                        .cloned()
                        .unwrap_or_default();
                    for cmd in cmds {
                        let bg_cmd = crate::commands::ensure_background(&cmd);
                        let _ = execute_command_string(&mut app, &bg_cmd);
                    }
                }
                if app.active_idx != _pre_status_idx && crate::debug_log::server_log_enabled() {
                    crate::debug_log::server_log(
                        "switch",
                        &format!(
                            "active_idx changed {} -> {} by status-interval hook",
                            _pre_status_idx, app.active_idx
                        ),
                    );
                }
            }
        }
        // ── Memory diagnostics (every 5s when enabled) ──
        {
            static LAST_MEM_LOG: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            if crate::debug_log::memory_log_enabled() {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let last = LAST_MEM_LOG.load(std::sync::atomic::Ordering::Relaxed);
                if now_ms.saturating_sub(last) >= 5000 {
                    LAST_MEM_LOG.store(now_ms, std::sync::atomic::Ordering::Relaxed);
                    let mem = crate::debug_log::process_memory_bytes();
                    let (receivers, frames, bytes) = crate::types::frame_push_stats();
                    let mode_str = match &app.mode {
                        Mode::CopyMode => "CopyMode",
                        Mode::CopySearch { .. } => "CopySearch",
                        Mode::Passthrough => "Passthrough",
                        _ => "Other",
                    };
                    let pane_count: usize = app
                        .windows
                        .iter()
                        .map(|w| {
                            fn count_panes(n: &crate::types::Node) -> usize {
                                match n {
                                    crate::types::Node::Leaf(_) => 1,
                                    crate::types::Node::Split { children, .. } => {
                                        children.iter().map(count_panes).sum()
                                    }
                                }
                            }
                            count_panes(&w.root)
                        })
                        .sum();
                    let shell_cache_size = app.shell_cmd_cache.lock().map(|c| c.len()).unwrap_or(0);
                    crate::debug_log::memory_log(
                        "heartbeat",
                        &format!(
                            "mode={} windows={} panes={} scroll_offset={} process_mem={} frame_receivers={} pushed_frames={} pushed_bytes={} shell_cache={}",
                            mode_str,
                            app.windows.len(),
                            pane_count,
                            app.copy_scroll_offset,
                            crate::debug_log::format_bytes(mem),
                            receivers, frames,
                            crate::debug_log::format_bytes(bytes),
                            shell_cache_size,
                        ),
                    );
                }
            }
        }
        // ── PaneChooser timeout ──
        // Auto-close display-panes overlay after display-panes-time (default 1000ms).
        if let Mode::PaneChooser { opened_at } = &app.mode {
            if opened_at.elapsed() > Duration::from_millis(app.display_panes_time_ms) {
                app.mode = Mode::Passthrough;
                state_dirty = true;
            }
        }
        // ── Hints mode timeout ──
        if let Mode::HintsMode(ref state) = app.mode {
            if app.hint_timeout > 0
                && state.entered_at.elapsed().as_millis() as u64 >= app.hint_timeout
            {
                app.mode = Mode::Passthrough;
                state_dirty = true;
            }
        }
        // ── Popup child exit detection ──
        // Check if popup PTY's child process has exited; if so, auto-close.
        if let Mode::PopupMode {
            ref mut popup_pane,
            close_on_exit,
            ..
        } = app.mode
        {
            let should_close = if let Some(ref mut pty) = popup_pane {
                matches!(pty.child.try_wait(), Ok(Some(_)))
            } else {
                false
            };
            if should_close && close_on_exit {
                app.mode = Mode::Passthrough;
                state_dirty = true;
            }
        }
        // ── display-panes auto-dismiss (#143/#144) ──
        // When display-panes is active in server/client mode, the server owns
        // PaneChooser mode but app::run()'s timeout check never fires.
        // Auto-dismiss here after display_panes_time_ms so pane numbers don't
        // stay on screen indefinitely.
        if let Mode::PaneChooser { opened_at } = &app.mode {
            if opened_at.elapsed() > Duration::from_millis(app.display_panes_time_ms) {
                app.mode = Mode::Passthrough;
                state_dirty = true;
            }
        }
        // Check if all windows/panes have exited (throttled to every 250ms)
        if last_reap.elapsed() >= Duration::from_millis(100) {
            last_reap = Instant::now();
            // Drain OSC 99/777 desktop notifications from panes
            helpers::drain_notifications(&mut app);
            // Check wait-pane waiters before reaping (so we can capture exit codes)
            if !app.wait_pane_queue.is_empty() {
                drain_wait_pane_queue(&mut app);
            }
            let (all_empty, any_pruned) = tree::reap_children(&mut app)?;
            if any_pruned {
                // A pane exited naturally - resize remaining panes to fill the space
                resize_all_panes(&mut app);
                state_dirty = true;
                meta_dirty = true;
                // Re-check waiters after reap: panes may have been pruned
                if !app.wait_pane_queue.is_empty() {
                    drain_wait_pane_queue(&mut app);
                }
            }
            // Fire context_ready push events for newly-ready panes
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            for win in &mut app.windows {
                tree::visit_leaves_mut(&mut win.root, &mut |p| {
                    if p.dead || p.readiness_notified {
                        return;
                    }
                    let dv = p.data_version.load(std::sync::atomic::Ordering::Acquire);
                    let lot = p
                        .last_output_time
                        .load(std::sync::atomic::Ordering::Acquire);
                    if dv > 0 && lot > 0 && now_ms.saturating_sub(lot) >= 500 {
                        p.readiness_notified = true;
                        let event = crate::backend::protocol::ContextReadyEvent {
                            method: "context_ready".into(),
                            params: crate::backend::protocol::ContextReadyParams {
                                context_id: format!("%{}", p.id),
                                ready_signal: "output_stable".into(),
                                data_version: dv,
                            },
                        };
                        if let Ok(json) = serde_json::to_string(&event) {
                            crate::types::push_backend_event(&json);
                        }
                        #[cfg(feature = "mycel")]
                        crate::mycel::publish_pane_event(
                            crate::mycel::topics::PANE_READY,
                            &serde_json::json!({
                                "pane_id": format!("%{}", p.id),
                                "elapsed_ms": p.spawn_time.elapsed().as_millis() as u64,
                            }),
                        );
                    }
                });
            }

            // Warm (standby) servers must always shut down when their
            // panes are gone, regardless of exit-empty / remain-on-exit
            // user config.  They are internal implementation details and
            // should never linger after all child processes have exited.
            let warm = is_warm_server(&app);
            let should_exit = if warm {
                all_empty || all_panes_dead(&mut app)
            } else {
                app.exit_empty && all_empty
            };
            if should_exit {
                let home = env::var("USERPROFILE")
                    .or_else(|_| env::var("HOME"))
                    .unwrap_or_default();
                let regpath = format!("{}\\.psmux\\{}.port", home, app.port_file_base());
                let keypath = format!("{}\\.psmux\\{}.key", home, app.port_file_base());
                let verpath = format!("{}\\.psmux\\{}.version", home, app.port_file_base());
                let _ = std::fs::remove_file(&regpath);
                let _ = std::fs::remove_file(&keypath);
                let _ = std::fs::remove_file(&verpath);
                let pipepath = format!("{}\\.psmux\\{}.pipe", home, app.port_file_base());
                let _ = std::fs::remove_file(&pipepath);
                crate::types::shutdown_persistent_streams();
                // Kill warm pane's child (process::exit skips Drop)
                if let Some(mut wp) = app.warm_pane.take() {
                    wp.child.kill().ok();
                }
                // When a non-warm session is the last one exiting, kill any
                // orphaned warm servers so they don't linger (#120, #138).
                if !warm {
                    let ns = app.socket_name.as_deref().map(|l| format!("{l}__"));
                    crate::session::kill_warm_servers(ns.as_deref());
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
                std::process::exit(0);
            }
        }
        // recv_timeout already handles the wait; no additional sleep needed.
    }
    #[allow(unreachable_code)]
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::should_spawn_warm_server;
    use crate::types::AppState;

    #[test]
    fn warm_server_is_disabled_for_destroy_unattached_sessions() {
        let mut app = AppState::new("demo".to_string());
        app.destroy_unattached = true;
        assert!(!should_spawn_warm_server(&app));
    }

    #[test]
    fn warm_server_is_disabled_for_warm_session_itself() {
        let app = AppState::new("__warm__".to_string());
        assert!(!should_spawn_warm_server(&app));
    }

    #[test]
    fn warm_server_is_allowed_for_normal_sessions() {
        let app = AppState::new("demo".to_string());
        assert!(should_spawn_warm_server(&app));
    }

    #[test]
    fn is_warm_server_detects_warm_session() {
        let app = AppState::new("__warm__".to_string());
        assert!(super::is_warm_server(&app));
    }

    #[test]
    fn is_warm_server_detects_namespaced_warm_session() {
        let mut app = AppState::new("__warm__".to_string());
        app.socket_name = Some("myns".to_string());
        assert!(super::is_warm_server(&app));
    }

    #[test]
    fn is_warm_server_rejects_normal_session() {
        let app = AppState::new("work".to_string());
        assert!(!super::is_warm_server(&app));
    }
}

#[cfg(test)]
#[path = "../../tests-rs/test_issue169_manual_rename.rs"]
mod test_issue169;
