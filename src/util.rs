use std::io;

use serde::{Deserialize, Serialize};

use crate::types::{AppState, Node};

/// Expand `~` to the user's home directory in a shell command string,
/// then rewrite `~/.psmux/plugins/` to `~/.config/psmux/plugins/` when
/// the classic path does not exist but the XDG path does (issue psmux-plugins#2).
pub fn expand_run_shell_path(cmd: &str) -> String {
    // Step 1: expand ~ to home directory
    let cmd = if cmd.contains('~') {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        cmd.replace("~/", &format!("{}/", home))
            .replace("~\\", &format!("{}\\", home))
    } else {
        cmd.to_string()
    };

    // Step 2: XDG fallback for plugin paths.
    // Both separators: the option value is user-written and either spelling
    // names the same directory on Windows.
    let home = crate::paths::home_dir();
    let plugins = crate::paths::psmux_dir_file("plugins");
    let classic_fwd = format!("{}/", plugins.replace('\\', "/"));
    let classic_win = format!("{}\\", plugins);
    if cmd.contains(&classic_fwd) || cmd.contains(&classic_win) {
        let classic_dir = std::path::PathBuf::from(&plugins);
        let xdg_base =
            std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}\\.config", home));
        let xdg_dir = std::path::Path::new(&xdg_base)
            .join("psmux")
            .join("plugins");
        if !classic_dir.is_dir() && xdg_dir.is_dir() {
            let xdg_fwd = format!("{}/psmux/plugins/", xdg_base.replace('\\', "/"));
            let xdg_win = format!("{}\\psmux\\plugins\\", xdg_base);
            cmd.replace(&classic_fwd, &xdg_fwd)
                .replace(&classic_win, &xdg_win)
        } else {
            cmd
        }
    } else {
        cmd
    }
}

/// Safe wrapper around `std::env::set_var` which became `unsafe` in Rust 1.83.
///
/// # Safety rationale
/// psmux is a single-threaded application (server and client loops run on one
/// thread each, and env vars are only mutated from the main thread of each
/// binary invocation).  No concurrent `env::var` reads race with these writes.
#[inline]
pub fn set_env(key: impl AsRef<std::ffi::OsStr>, value: impl AsRef<std::ffi::OsStr>) {
    // SAFETY: see doc comment above — no concurrent env readers.
    unsafe {
        std::env::set_var(key, value);
    }
}

#[inline]
pub fn remove_env(key: impl AsRef<std::ffi::OsStr>) {
    // SAFETY: see doc comment above — no concurrent env readers.
    unsafe {
        std::env::remove_var(key);
    }
}

pub fn infer_title_from_prompt(screen: &vt100::Screen, rows: u16, cols: u16) -> Option<String> {
    // Scan from cursor row (most likely prompt location) then fall back to last non-empty row
    let cursor_row = screen.cursor_position().0;
    let mut candidate_row: Option<u16> = None;
    // Try cursor row first, then scan downward, then scan upward
    for &r in [cursor_row]
        .iter()
        .chain((cursor_row + 1..rows).collect::<Vec<_>>().iter())
        .chain((0..cursor_row).rev().collect::<Vec<_>>().iter())
    {
        let mut s = String::new();
        for c in 0..cols {
            if let Some(cell) = screen.cell(r, c) {
                s.push_str(cell.contents());
            } else {
                s.push(' ');
            }
        }
        let t = s.trim_end();
        if !t.is_empty()
            && (t.contains('>') || t.contains('$') || t.contains('#') || t.contains(':'))
        {
            candidate_row = Some(r);
            break;
        }
    }
    // Fall back: use the row the cursor is on even if no prompt marker
    let row = candidate_row.unwrap_or(cursor_row);
    let mut s = String::new();
    for c in 0..cols {
        if let Some(cell) = screen.cell(row, c) {
            s.push_str(cell.contents());
        } else {
            s.push(' ');
        }
    }
    let trimmed = s.trim().to_string();
    if trimmed.is_empty() {
        return None;
    }
    // Only infer title from lines that look like prompts (contain a prompt marker)
    let has_prompt_marker =
        trimmed.contains('>') || trimmed.ends_with('$') || trimmed.ends_with('#');
    if !has_prompt_marker {
        // If no prompt marker, don't change the title — this is likely command output
        return None;
    }
    if let Some(pos) = trimmed.rfind('>') {
        let before = trimmed[..pos].trim().to_string();
        if before.contains("\\") || before.contains("/") {
            let parts: Vec<&str> = before
                .trim_matches(|ch: char| ch == '"')
                .split(['\\', '/'])
                .collect();
            if let Some(base) = parts.last() {
                return Some(base.to_string());
            }
        }
        return Some(before);
    }
    if let Some(pos) = trimmed.rfind('$') {
        return Some(trimmed[..pos].trim().to_string());
    }
    if let Some(pos) = trimmed.rfind('#') {
        return Some(trimmed[..pos].trim().to_string());
    }
    Some(trimmed)
}

// resolve_last_session_name and resolve_default_session_name are in session.rs

#[derive(Serialize, Deserialize)]
pub struct WinInfo {
    pub id: usize,
    pub name: String,
    pub active: bool,
    #[serde(default)]
    pub activity: bool,
    #[serde(default)]
    pub tab_text: String,
}

// ─────────────────── --json output structs ───────────────────────

/// Structured JSON output for `list-sessions --json`.
#[derive(Serialize)]
pub struct SessionJsonInfo {
    pub name: String,
    pub windows: usize,
    pub attached: bool,
    pub created: String,
}

/// Structured JSON output for `list-panes --json`.
#[derive(Serialize)]
pub struct PaneJsonInfo {
    pub pane_id: String,
    pub window_index: usize,
    pub pane_index: usize,
    pub width: u16,
    pub height: u16,
    pub active: bool,
    pub pid: Option<u32>,
    pub current_path: String,
    pub title: String,
}

/// Structured JSON output for `list-windows --json`.
#[derive(Serialize)]
pub struct WindowJsonInfo {
    pub index: usize,
    pub name: String,
    pub layout: String,
    pub active: bool,
    pub panes: usize,
    pub width: u16,
    pub height: u16,
}

/// Structured JSON output for `display-message --json`.
#[derive(Serialize)]
pub struct DisplayMessageJson {
    pub message: String,
}

/// Structured JSON output for `capture-pane --json`.
#[derive(Serialize)]
pub struct CapturePaneJson {
    pub pane_id: String,
    pub content: String,
}

#[derive(Serialize, Deserialize)]
pub struct PaneInfo {
    pub id: usize,
    pub title: String,
}

#[derive(Serialize, Deserialize)]
pub struct WinTree {
    pub id: usize,
    pub name: String,
    pub active: bool,
    pub panes: Vec<PaneInfo>,
}

pub fn list_windows_json(app: &AppState) -> io::Result<String> {
    let mut v: Vec<WinInfo> = Vec::new();
    for (i, w) in app.windows.iter().enumerate() {
        v.push(WinInfo {
            id: w.id,
            name: w.name.clone(),
            active: i == app.active_idx,
            activity: w.activity_flag,
            tab_text: String::new(),
        });
    }
    let s = serde_json::to_string(&v).map_err(|e| io::Error::other(format!("json error: {e}")))?;
    Ok(s)
}

/// tmux-compatible list-windows output: one line per window
/// Format: `<index>: <name><flag> (<pane_count> panes) [<width>x<height>]`
pub fn list_windows_tmux(app: &AppState) -> String {
    use crate::tree::*;
    fn count_panes(node: &Node) -> usize {
        match node {
            Node::Leaf(_) => 1,
            Node::Split { children, .. } => children.iter().map(count_panes).sum(),
        }
    }
    let mut lines = Vec::new();
    for (i, w) in app.windows.iter().enumerate() {
        let flag = if i == app.active_idx {
            "*"
        } else if w.activity_flag {
            "#"
        } else {
            "-"
        };
        let pane_count = count_panes(&w.root);
        let (width, height) = if let Some(p) = active_pane(&w.root, &w.active_path) {
            (p.last_cols, p.last_rows)
        } else {
            (120, 30)
        };
        lines.push(format!(
            "{}: {}{} ({} panes) [{}x{}]",
            i + app.window_base_index,
            w.name,
            flag,
            pane_count,
            width,
            height
        ));
    }
    lines.join("\n")
}

pub fn list_tree_json(app: &AppState) -> io::Result<String> {
    fn collect_panes(node: &Node, out: &mut Vec<PaneInfo>) {
        match node {
            Node::Leaf(p) => {
                out.push(PaneInfo {
                    id: p.id,
                    title: p.title.clone(),
                });
            }
            Node::Split { children, .. } => {
                for c in children.iter() {
                    collect_panes(c, out);
                }
            }
        }
    }
    let mut v: Vec<WinTree> = Vec::new();
    for (i, w) in app.windows.iter().enumerate() {
        let mut panes = Vec::new();
        collect_panes(&w.root, &mut panes);
        v.push(WinTree {
            id: w.id,
            name: w.name.clone(),
            active: i == app.active_idx,
            panes,
        });
    }
    let s = serde_json::to_string(&v).map_err(|e| io::Error::other(format!("json error: {e}")))?;
    Ok(s)
}

pub const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &str) -> String {
    let bytes = data.as_bytes();
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        result.push(BASE64_CHARS[b0 >> 2] as char);
        result.push(BASE64_CHARS[((b0 & 0x03) << 4) | (b1 >> 4)] as char);
        if chunk.len() > 1 {
            result.push(BASE64_CHARS[((b1 & 0x0f) << 2) | (b2 >> 6)] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(BASE64_CHARS[b2 & 0x3f] as char);
        } else {
            result.push('=');
        }
    }
    result
}

pub fn base64_decode(encoded: &str) -> Option<String> {
    let mut result = Vec::new();
    let chars: Vec<u8> = encoded.bytes().filter(|&b| b != b'=').collect();
    for chunk in chars.chunks(4) {
        if chunk.len() < 2 {
            break;
        }
        let b0 = BASE64_CHARS.iter().position(|&c| c == chunk[0])? as u8;
        let b1 = BASE64_CHARS.iter().position(|&c| c == chunk[1])? as u8;
        result.push((b0 << 2) | (b1 >> 4));
        if chunk.len() > 2 {
            let b2 = BASE64_CHARS.iter().position(|&c| c == chunk[2])? as u8;
            result.push((b1 << 4) | (b2 >> 2));
            if chunk.len() > 3 {
                let b3 = BASE64_CHARS.iter().position(|&c| c == chunk[3])? as u8;
                result.push((b2 << 6) | b3);
            }
        }
    }
    String::from_utf8(result).ok()
}

/// Return color name as a string. Uses static strings for Default and
/// the 256 indexed colors to avoid heap allocations on every cell.
/// Quote and escape an argument for safe transmission over the control protocol.
/// Wraps the value in double quotes and escapes any embedded double quotes or backslashes.
pub fn quote_arg(s: &str) -> String {
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{}\"", escaped)
}

/// Quote an argument for the control wire, but only when it needs it.
///
/// The wire is line-oriented and the server reads one command per `read_line`,
/// so any argument carrying a line terminator must be quoted and escaped or the
/// line is cut and the tail is dispatched as a fresh command against the
/// caller's session (#560). `char::is_whitespace` covers `\n` and `\r` as well
/// as space and tab, which is exactly the set that must never travel raw.
///
/// A value that needs nothing — the common case, including a Windows path such
/// as `C:\node_modules` — is returned byte-exact so the wire stays readable and
/// existing behaviour is unchanged.
pub fn quote_arg_if_needed(s: &str) -> String {
    if s.is_empty() || s.chars().any(|c| c.is_whitespace() || c == '"') {
        quote_arg(s)
    } else {
        s.to_string()
    }
}

/// Flatten send-keys key arguments onto the control wire, space separated.
///
/// Every argument goes through [`quote_arg_if_needed`], so no argument can
/// contribute a raw line terminator. The result is guaranteed to contain no
/// `\n` or `\r`, which is the whole property #560 turns on: the caller appends
/// exactly one `\n` as the line terminator, and the server's `read_line`
/// therefore sees the entire command.
pub fn flatten_send_keys_args(keys: &[String]) -> String {
    let mut out = String::new();
    for k in keys {
        out.push(' ');
        out.push_str(&quote_arg_if_needed(k));
    }
    out
}

#[cfg(test)]
mod tests {

    // ---- issue #560: line terminators must never travel raw on the wire ----
    //
    // The server reads one command per read_line. A raw 0x0A inside a send-keys
    // payload cut the line there, and the tail was dispatched as a fresh psmux
    // command against the caller's session, while the client exited 0.

    #[test]
    fn quote_arg_escapes_newline() {
        let out = quote_arg("TEST_HEAD\nrename-window pwned");
        assert!(!out.contains('\n'), "raw 0x0A must not survive: {:?}", out);
        assert!(out.contains("\\n"), "newline must become \\n: {:?}", out);
    }

    #[test]
    fn quote_arg_escapes_carriage_return() {
        let out = quote_arg("CRHEAD\rCRTAIL");
        assert!(!out.contains('\r'), "raw 0x0D must not survive: {:?}", out);
        assert!(
            out.contains("\\r"),
            "carriage return must become \\r: {:?}",
            out
        );
    }

    #[test]
    fn quote_arg_escapes_backslash_before_introducing_its_own() {
        // Backslash first, or the escapes introduced below would be doubled.
        assert_eq!(quote_arg("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }

    #[test]
    fn a_windows_path_round_trips_through_the_wire_encoding() {
        // The reason the decoder is deliberately NOT taught a global \n rule:
        // this path would be corrupted by one. Encode with quote_arg, decode
        // with the real server-side parser, and require it back byte-exact.
        let path = r"C:\node_modules\.bin";
        let line = format!("send-keys {}", quote_arg(path));
        let parsed = crate::commands::parse_command_line(&line);
        assert_eq!(
            parsed.len(),
            2,
            "expected verb + one argument: {:?}",
            parsed
        );
        assert_eq!(parsed[1], path, "Windows path must survive the round trip");
    }

    #[test]
    fn quote_arg_if_needed_quotes_a_payload_carrying_a_newline() {
        let out = quote_arg_if_needed("TEST_HEAD\nrename-window pwned");
        assert!(
            !out.contains('\n'),
            "a newline payload must not reach the wire raw: {:?}",
            out
        );
        assert!(out.starts_with('"'), "it must be quoted: {:?}", out);
    }

    #[test]
    fn quote_arg_if_needed_quotes_a_payload_carrying_a_carriage_return() {
        let out = quote_arg_if_needed("A\rB");
        assert!(
            !out.contains('\r'),
            "raw 0x0D must not reach the wire: {:?}",
            out
        );
    }

    #[test]
    fn quote_arg_if_needed_quotes_whitespace_and_quotes() {
        assert!(quote_arg_if_needed("two words").starts_with('"'));
        assert!(quote_arg_if_needed("tab\there").starts_with('"'));
        assert!(quote_arg_if_needed("say \"hi\"").starts_with('"'));
        assert_eq!(quote_arg_if_needed(""), "\"\"");
    }

    #[test]
    fn send_keys_flattening_never_emits_a_raw_line_terminator() {
        // The #560 contract. The caller appends exactly one '\n' as the wire
        // terminator, so if flattening contributes one of its own the server's
        // read_line cuts the command there and dispatches the tail.
        let keys = vec!["TEST_HEAD\nrename-window pwned".to_string()];
        let flat = flatten_send_keys_args(&keys);
        // Both halves matter. Without the second assertion an empty result
        // satisfies the first one vacuously, which proves nothing.
        assert!(
            !flat.contains('\n') && !flat.contains('\r'),
            "flattened args must carry no raw line terminator: {:?}",
            flat
        );
        assert!(
            flat.contains("TEST_HEAD") && flat.contains("rename-window pwned"),
            "the payload must still be delivered, just not executable: {:?}",
            flat
        );
    }

    #[test]
    fn send_keys_flattening_survives_a_newline_without_other_whitespace() {
        // The original guard was `contains(' ') || contains('\t') || contains('"')`,
        // so a payload whose only whitespace is the newline took the unquoted
        // branch and went onto the wire completely raw.
        let keys = vec!["HEAD\nkill-session".to_string()];
        let flat = flatten_send_keys_args(&keys);
        assert!(
            !flat.contains('\n'),
            "unquoted branch leaked a newline: {:?}",
            flat
        );
        assert!(
            flat.contains("HEAD") && flat.contains("kill-session"),
            "the payload must still be delivered: {:?}",
            flat
        );
    }

    #[test]
    fn send_keys_flattening_keeps_plain_arguments_unquoted() {
        // Guard on the fix: quoting everything would change the wire for every
        // existing send-keys caller.
        let keys = vec!["echo".to_string(), "hi".to_string(), "Enter".to_string()];
        assert_eq!(flatten_send_keys_args(&keys), " echo hi Enter");
    }

    #[test]
    fn send_keys_flattening_quotes_an_argument_with_spaces() {
        let keys = vec!["two words".to_string()];
        assert_eq!(flatten_send_keys_args(&keys), " \"two words\"");
    }

    #[test]
    fn quote_arg_if_needed_leaves_a_plain_argument_byte_exact() {
        // Guard on the fix: quoting everything would change the wire for every
        // existing caller. A Windows path with no whitespace must pass through.
        assert_eq!(quote_arg_if_needed("Enter"), "Enter");
        assert_eq!(quote_arg_if_needed(r"C:\node_modules"), r"C:\node_modules");
        assert_eq!(quote_arg_if_needed("--flag=value"), "--flag=value");
    }
    use super::*;
    use crate::commands::parse_command_line;

    #[test]
    fn test_quote_arg_simple() {
        assert_eq!(quote_arg("hello"), "\"hello\"");
    }

    #[test]
    fn test_quote_arg_with_spaces() {
        assert_eq!(quote_arg("cc 123"), "\"cc 123\"");
    }

    #[test]
    fn test_quote_arg_with_embedded_quotes() {
        assert_eq!(quote_arg("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn test_quote_arg_with_backslash() {
        assert_eq!(quote_arg("C:\\Users\\foo"), "\"C:\\\\Users\\\\foo\"");
    }

    #[test]
    fn test_quote_arg_empty() {
        assert_eq!(quote_arg(""), "\"\"");
    }

    #[test]
    fn test_rename_session_roundtrip_with_spaces() {
        let name = "cc 123";
        let cmd = format!("rename-session {}", quote_arg(name));
        let args = parse_command_line(&cmd);
        assert_eq!(args, vec!["rename-session", "cc 123"]);
    }

    #[test]
    fn test_rename_window_roundtrip_with_spaces() {
        let name = "my window";
        let cmd = format!("rename-window {}", quote_arg(name));
        let args = parse_command_line(&cmd);
        assert_eq!(args, vec!["rename-window", "my window"]);
    }

    #[test]
    fn test_set_pane_title_roundtrip_with_spaces() {
        let title = "pane title here";
        let cmd = format!("set-pane-title {}", quote_arg(title));
        let args = parse_command_line(&cmd);
        assert_eq!(args, vec!["set-pane-title", "pane title here"]);
    }

    #[test]
    fn test_source_file_roundtrip_windows_path_with_spaces() {
        let path = "C:\\Program Files\\psmux\\config.conf";
        let cmd = format!("source-file {}", quote_arg(path));
        let args = parse_command_line(&cmd);
        assert_eq!(
            args,
            vec!["source-file", "C:\\Program Files\\psmux\\config.conf"]
        );
    }

    #[test]
    fn test_claim_session_roundtrip_with_spaces() {
        let name = "my session";
        let cwd = "C:\\Users\\My Name\\Documents";
        let cmd = format!("claim-session {} {}", quote_arg(name), quote_arg(cwd));
        let args = parse_command_line(&cmd);
        assert_eq!(
            args,
            vec![
                "claim-session",
                "my session",
                "C:\\Users\\My Name\\Documents"
            ]
        );
    }

    #[test]
    fn test_roundtrip_name_with_embedded_quotes() {
        let name = "say \"hello\" world";
        let cmd = format!("rename-session {}", quote_arg(name));
        let args = parse_command_line(&cmd);
        assert_eq!(args, vec!["rename-session", "say \"hello\" world"]);
    }

    #[test]
    fn test_roundtrip_no_spaces_still_works() {
        let name = "simple";
        let cmd = format!("rename-session {}", quote_arg(name));
        let args = parse_command_line(&cmd);
        assert_eq!(args, vec!["rename-session", "simple"]);
    }

    #[test]
    fn test_claim_session_roundtrip_root_dir() {
        // Root paths like C:\ end in a backslash which must survive
        // the quote_arg -> parse_command_line roundtrip.
        let name = "mysession";
        let cwd = "C:\\";
        let cmd = format!("claim-session {} {}", quote_arg(name), quote_arg(cwd));
        let args = parse_command_line(&cmd);
        assert_eq!(args, vec!["claim-session", "mysession", "C:\\"]);
    }

    #[test]
    fn test_claim_session_roundtrip_trailing_backslash_dir() {
        // Paths ending in backslash (e.g. D:\Projects\) must roundtrip.
        let cwd = "D:\\Projects\\";
        let cmd = format!("claim-session sess {}", quote_arg(cwd));
        let args = parse_command_line(&cmd);
        assert_eq!(args, vec!["claim-session", "sess", "D:\\Projects\\"]);
    }

    #[test]
    fn test_claim_session_roundtrip_path_with_spaces() {
        let cwd = "C:\\Program Files\\My App\\Data";
        let cmd = format!("claim-session s1 {}", quote_arg(cwd));
        let args = parse_command_line(&cmd);
        assert_eq!(
            args,
            vec!["claim-session", "s1", "C:\\Program Files\\My App\\Data"]
        );
    }

    #[test]
    fn test_claim_session_roundtrip_deep_nested_path() {
        let cwd = "C:\\Users\\test\\Documents\\workspace\\project\\src\\components";
        let cmd = format!("claim-session s1 {}", quote_arg(cwd));
        let args = parse_command_line(&cmd);
        assert_eq!(args, vec!["claim-session", "s1", cwd]);
    }

    #[test]
    fn test_claim_session_roundtrip_unc_path() {
        let cwd = "\\\\server\\share\\folder";
        let cmd = format!("claim-session s1 {}", quote_arg(cwd));
        let args = parse_command_line(&cmd);
        assert_eq!(
            args,
            vec!["claim-session", "s1", "\\\\server\\share\\folder"]
        );
    }

    #[test]
    fn test_claim_session_roundtrip_path_with_parens() {
        let cwd = "C:\\Program Files (x86)\\App";
        let cmd = format!("claim-session s1 {}", quote_arg(cwd));
        let args = parse_command_line(&cmd);
        assert_eq!(
            args,
            vec!["claim-session", "s1", "C:\\Program Files (x86)\\App"]
        );
    }

    #[test]
    fn test_claim_session_roundtrip_path_with_ampersand() {
        let cwd = "C:\\R&D\\project";
        let cmd = format!("claim-session s1 {}", quote_arg(cwd));
        let args = parse_command_line(&cmd);
        assert_eq!(args, vec!["claim-session", "s1", "C:\\R&D\\project"]);
    }

    /// Verify that send-keys with Claude Code agent spawn commands preserves
    /// Windows paths and POSIX-escaped characters (psmux#172, #173, #180).
    /// The CLI wraps the key in double-quotes without escaping backslashes,
    /// and parse_command_line keeps lone backslashes literal (Windows paths).
    #[test]
    fn test_send_keys_claude_code_agent_command_preserves_backslashes() {
        // Simulate the control-protocol line built by the CLI send-keys handler:
        // send-keys "cd 'C:\path with spaces' && env CLAUDECODE=1 'C:\...\claude.exe' --agent-id ..." Enter
        let agent_cmd = "cd 'C:\\cctest\\a long dir name' && env CLAUDECODE=1 'C:\\Users\\foo\\.local\\bin\\claude.exe' --agent-id researcher\\@my-team";
        let line = format!("send-keys \"{}\" Enter", agent_cmd);
        let args = parse_command_line(&line);
        assert_eq!(args[0], "send-keys");
        assert_eq!(args[1], agent_cmd);
        assert_eq!(args[2], "Enter");
    }

    #[test]
    fn test_send_keys_single_quoted_windows_path() {
        // Single-quoted paths from shell-quote: 'C:\Users\foo'
        let line = "send-keys \"cd 'C:\\Users\\foo\\project'\" Enter";
        let args = parse_command_line(line);
        assert_eq!(args[1], "cd 'C:\\Users\\foo\\project'");
    }
}

pub fn color_to_name(c: vt100::Color) -> std::borrow::Cow<'static, str> {
    use std::borrow::Cow;
    match c {
        vt100::Color::Default => Cow::Borrowed("default"),
        vt100::Color::Idx(i) => {
            // Static lookup table for all 256 indexed colors
            static IDX_STRINGS: std::sync::LazyLock<[String; 256]> =
                std::sync::LazyLock::new(|| std::array::from_fn(|i| format!("idx:{}", i)));
            Cow::Borrowed(&IDX_STRINGS[i as usize])
        }
        vt100::Color::Rgb(r, g, b) => Cow::Owned(format!("rgb:{},{},{}", r, g, b)),
    }
}
