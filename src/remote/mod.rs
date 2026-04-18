pub mod pane_manager;
pub mod parser;
pub mod protocol;
pub mod ssh;

use std::io::Write;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;

use pane_manager::RemotePaneManager;
use parser::ControlModeParser;
use ssh::SshTransport;

/// Messages sent from the SSH reader background thread to the main event loop.
enum SshEvent {
    /// A complete line was read from SSH stdout.
    Line(String),
    /// SSH stdout reached EOF — the remote tmux session has ended.
    Eof,
    /// A read error occurred on the SSH connection.
    Error(String),
}

/// Translate a crossterm key event into a tmux `send-keys` command.
///
/// Returns `None` for key events that cannot be meaningfully forwarded
/// (e.g. unknown/modifier-only keys).
fn key_to_tmux_send_keys(key: &crossterm::event::KeyEvent) -> Option<String> {
    // Build modifier prefix: C- for Ctrl, M- for Alt, S- for Shift.
    let mut prefix = String::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        prefix.push_str("C-");
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        prefix.push_str("M-");
    }

    match key.code {
        KeyCode::Enter => Some("send-keys Enter".to_string()),
        KeyCode::Backspace => Some("send-keys BSpace".to_string()),
        KeyCode::Tab => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                Some("send-keys BTab".to_string())
            } else {
                Some("send-keys Tab".to_string())
            }
        }
        KeyCode::BackTab => Some("send-keys BTab".to_string()),
        KeyCode::Esc => Some("send-keys Escape".to_string()),
        KeyCode::Up => Some(format!("send-keys {}Up", prefix)),
        KeyCode::Down => Some(format!("send-keys {}Down", prefix)),
        KeyCode::Left => Some(format!("send-keys {}Left", prefix)),
        KeyCode::Right => Some(format!("send-keys {}Right", prefix)),
        KeyCode::Home => Some(format!("send-keys {}Home", prefix)),
        KeyCode::End => Some(format!("send-keys {}End", prefix)),
        KeyCode::PageUp => Some(format!("send-keys {}PageUp", prefix)),
        KeyCode::PageDown => Some(format!("send-keys {}PageDown", prefix)),
        KeyCode::Delete => Some(format!("send-keys {}DC", prefix)),
        KeyCode::Insert => Some(format!("send-keys {}IC", prefix)),
        KeyCode::F(n) => Some(format!("send-keys {}F{}", prefix, n)),
        KeyCode::Char(' ') => Some("send-keys Space".to_string()),
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                Some(format!("send-keys C-{}", c))
            } else if key.modifiers.contains(KeyModifiers::ALT) {
                Some(format!("send-keys M-{}", c))
            } else {
                // Regular character — quote it for tmux.
                // Semicolons and backslashes need escaping in tmux commands.
                match c {
                    ';' => Some("send-keys \\;".to_string()),
                    '\\' => Some("send-keys \\\\".to_string()),
                    '\'' => Some("send-keys \"'\"".to_string()),
                    _ => Some(format!("send-keys '{}'", c)),
                }
            }
        }
        _ => None,
    }
}

/// Render the active pane's vt100 screen contents to stdout.
///
/// Uses `contents_formatted()` which emits a complete VT byte stream
/// including cursor positioning, attributes (colors, bold, etc.), and content.
/// We prepend a cursor-home sequence so the output overwrites the terminal
/// from position (1,1).
fn render_active_pane(
    manager: &RemotePaneManager,
    stdout: &mut impl Write,
    _rows: u16,
    _cols: u16,
) -> std::io::Result<()> {
    if let Some(pane_parser) = manager.get_active_pane() {
        if let Ok(p) = pane_parser.lock() {
            let screen = p.screen();
            // Cursor home — position (1,1)
            stdout.write_all(b"\x1b[H")?;
            // Write the full formatted screen (includes cursor positioning,
            // colors, and content — row by row with proper VT sequences)
            stdout.write_all(&screen.contents_formatted())?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// Main entry point for remote tmux session rendering.
///
/// Connects to a remote host via SSH, starts (or attaches to) a tmux session
/// in control mode (`-CC`), and provides an interactive terminal: keyboard
/// input is forwarded as tmux `send-keys` commands, terminal resizes trigger
/// `refresh-client`, and the active pane's vt100 screen is rendered to stdout.
///
/// The SSH stdout reader runs on a background thread to avoid blocking the
/// main event loop, which polls for both keyboard events and incoming SSH data.
pub fn run_remote_tmux(
    ssh_target: &str,
    session: Option<&str>,
    ssh_opts: Option<&str>,
) -> std::io::Result<()> {
    let session_name = session.unwrap_or("default");

    // Try attach first, fall back to new-session
    let mut transport = SshTransport::connect(ssh_target, session_name, true, ssh_opts)
        .or_else(|_| SshTransport::connect(ssh_target, session_name, false, ssh_opts))?;

    let (cols, rows) = terminal::size()?;
    transport.send_command(&format!("refresh-client -C {}x{}", cols, rows))?;

    // Split transport: reader goes to background thread, writer stays in main
    let (mut ssh_reader, mut ssh_writer) = transport.split();
    let (line_tx, line_rx) = mpsc::channel::<SshEvent>();

    // Background thread: read lines from SSH stdout and send via channel
    thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match ssh_reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = line_tx.send(SshEvent::Eof);
                    break;
                }
                Ok(_) => {
                    if line_tx.send(SshEvent::Line(line.clone())).is_err() {
                        break; // Main thread dropped the receiver
                    }
                }
                Err(e) => {
                    let _ = line_tx.send(SshEvent::Error(e.to_string()));
                    break;
                }
            }
        }
    });

    // Enter raw mode and alternate screen for keyboard capture
    terminal::enable_raw_mode()?;

    // Use a closure-based cleanup pattern to ensure raw mode + alternate screen
    // are always restored, even on error or early return.
    let result = run_interactive_loop(&mut ssh_writer, &line_rx, cols, rows, session_name);

    // Cleanup: always restore terminal state
    let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen,);
    let _ = terminal::disable_raw_mode();

    // Disconnect the SSH session (writer owns the child process)
    ssh_writer.disconnect();

    match &result {
        Ok(()) => println!("[detached from remote session '{}']", session_name),
        Err(e) => eprintln!("[remote session '{}' error: {}]", session_name, e),
    }

    result
}

/// Inner interactive event loop, separated for clean error handling.
///
/// Called after raw mode is enabled; the caller is responsible for restoring
/// terminal state regardless of the return value.
fn run_interactive_loop(
    ssh_writer: &mut ssh::SshWriter,
    line_rx: &mpsc::Receiver<SshEvent>,
    initial_cols: u16,
    initial_rows: u16,
    _session_name: &str,
) -> std::io::Result<()> {
    crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen,)?;

    let mut stdout = std::io::stdout();
    let mut cm_parser = ControlModeParser::new();
    let mut manager = RemotePaneManager::new(initial_cols, initial_rows);
    let mut last_cols = initial_cols;
    let mut last_rows = initial_rows;
    let mut needs_render = false;
    // Track whether we are waiting for the first pane to appear (initial layout).
    // Before we have an active pane, rendering is a no-op.
    let mut has_active_pane = false;
    // Prefix key state: Ctrl+b is the tmux default prefix.
    let mut prefix_armed = false;

    loop {
        // ── Poll for keyboard events with a short timeout ────────────────
        // This determines the maximum latency for rendering updates from SSH.
        if event::poll(Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(key) => {
                    // Only act on Press/Repeat, ignore Release events
                    if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
                        // Skip release events
                    } else if prefix_armed {
                        // We're in prefix mode — interpret the next key
                        prefix_armed = false;
                        match key.code {
                            KeyCode::Char('d') => {
                                // Detach from the remote session
                                return Ok(());
                            }
                            _ => {
                                // Forward the prefix key itself and then this key
                                // to the remote tmux. The remote tmux also has a
                                // prefix key, so send Ctrl+b followed by the key.
                                ssh_writer.send_command("send-keys C-b")?;
                                if let Some(cmd) = key_to_tmux_send_keys(&key) {
                                    ssh_writer.send_command(&cmd)?;
                                }
                            }
                        }
                    } else if key.code == KeyCode::Char('b')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        // Prefix key pressed — arm prefix mode
                        prefix_armed = true;
                    } else if let Some(cmd) = key_to_tmux_send_keys(&key) {
                        ssh_writer.send_command(&cmd)?;
                    }
                }
                Event::Resize(w, h) => {
                    if w != last_cols || h != last_rows {
                        last_cols = w;
                        last_rows = h;
                        ssh_writer.send_command(&format!("refresh-client -C {}x{}", w, h))?;
                        needs_render = true;
                    }
                }
                _ => {
                    // Mouse events, paste events, etc. — ignore for now
                }
            }
        }

        // ── Drain SSH lines from the background reader ──────────────────
        let mut got_data = false;
        loop {
            match line_rx.try_recv() {
                Ok(SshEvent::Line(line)) => {
                    if let Some(msg) = cm_parser.feed_line(&line) {
                        if matches!(msg, protocol::ControlModeMessage::Exit { .. }) {
                            return Ok(());
                        }
                        // Track when we first get an active pane
                        let is_output = matches!(
                            msg,
                            protocol::ControlModeMessage::Output { .. }
                                | protocol::ControlModeMessage::ExtendedOutput { .. }
                        );
                        let is_layout =
                            matches!(msg, protocol::ControlModeMessage::LayoutChange { .. });
                        let is_pane_change =
                            matches!(msg, protocol::ControlModeMessage::WindowPaneChanged { .. });
                        manager.handle_message(msg);
                        if is_layout || is_pane_change {
                            has_active_pane = manager.get_active_pane().is_some();
                        }
                        if is_output || is_layout || is_pane_change {
                            got_data = true;
                        }
                    }
                }
                Ok(SshEvent::Eof) => {
                    return Ok(());
                }
                Ok(SshEvent::Error(e)) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        e,
                    ));
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Ok(());
                }
            }
        }

        // ── Render the active pane ──────────────────────────────────────
        if (got_data || needs_render) && has_active_pane {
            render_active_pane(&manager, &mut stdout, last_rows, last_cols)?;
            needs_render = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyEventState};

    /// Helper to create a KeyEvent for testing.
    fn make_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn test_key_to_tmux_enter() {
        let key = make_key(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys Enter");
    }

    #[test]
    fn test_key_to_tmux_backspace() {
        let key = make_key(KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys BSpace");
    }

    #[test]
    fn test_key_to_tmux_tab() {
        let key = make_key(KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys Tab");
    }

    #[test]
    fn test_key_to_tmux_shift_tab() {
        let key = make_key(KeyCode::Tab, KeyModifiers::SHIFT);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys BTab");
    }

    #[test]
    fn test_key_to_tmux_backtab() {
        let key = make_key(KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys BTab");
    }

    #[test]
    fn test_key_to_tmux_escape() {
        let key = make_key(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys Escape");
    }

    #[test]
    fn test_key_to_tmux_arrows() {
        assert_eq!(
            key_to_tmux_send_keys(&make_key(KeyCode::Up, KeyModifiers::NONE)).unwrap(),
            "send-keys Up"
        );
        assert_eq!(
            key_to_tmux_send_keys(&make_key(KeyCode::Down, KeyModifiers::NONE)).unwrap(),
            "send-keys Down"
        );
        assert_eq!(
            key_to_tmux_send_keys(&make_key(KeyCode::Left, KeyModifiers::NONE)).unwrap(),
            "send-keys Left"
        );
        assert_eq!(
            key_to_tmux_send_keys(&make_key(KeyCode::Right, KeyModifiers::NONE)).unwrap(),
            "send-keys Right"
        );
    }

    #[test]
    fn test_key_to_tmux_ctrl_arrow() {
        let key = make_key(KeyCode::Up, KeyModifiers::CONTROL);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys C-Up");
    }

    #[test]
    fn test_key_to_tmux_ctrl_char() {
        let key = make_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys C-c");
    }

    #[test]
    fn test_key_to_tmux_alt_char() {
        let key = make_key(KeyCode::Char('x'), KeyModifiers::ALT);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys M-x");
    }

    #[test]
    fn test_key_to_tmux_regular_char() {
        let key = make_key(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys 'a'");
    }

    #[test]
    fn test_key_to_tmux_space() {
        let key = make_key(KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys Space");
    }

    #[test]
    fn test_key_to_tmux_semicolon() {
        let key = make_key(KeyCode::Char(';'), KeyModifiers::NONE);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys \\;");
    }

    #[test]
    fn test_key_to_tmux_function_key() {
        let key = make_key(KeyCode::F(12), KeyModifiers::NONE);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys F12");
    }

    #[test]
    fn test_key_to_tmux_delete() {
        let key = make_key(KeyCode::Delete, KeyModifiers::NONE);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys DC");
    }

    #[test]
    fn test_key_to_tmux_insert() {
        let key = make_key(KeyCode::Insert, KeyModifiers::NONE);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys IC");
    }

    #[test]
    fn test_key_to_tmux_page_up() {
        let key = make_key(KeyCode::PageUp, KeyModifiers::NONE);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys PageUp");
    }

    #[test]
    fn test_key_to_tmux_home_end() {
        assert_eq!(
            key_to_tmux_send_keys(&make_key(KeyCode::Home, KeyModifiers::NONE)).unwrap(),
            "send-keys Home"
        );
        assert_eq!(
            key_to_tmux_send_keys(&make_key(KeyCode::End, KeyModifiers::NONE)).unwrap(),
            "send-keys End"
        );
    }

    #[test]
    fn test_key_to_tmux_backslash() {
        let key = make_key(KeyCode::Char('\\'), KeyModifiers::NONE);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys \\\\");
    }

    #[test]
    fn test_key_to_tmux_single_quote() {
        let key = make_key(KeyCode::Char('\''), KeyModifiers::NONE);
        assert_eq!(key_to_tmux_send_keys(&key).unwrap(), "send-keys \"'\"");
    }

    #[test]
    fn test_render_active_pane_no_active_pane() {
        let manager = RemotePaneManager::new(80, 24);
        let mut buf: Vec<u8> = Vec::new();
        // Should not error when there's no active pane
        render_active_pane(&manager, &mut buf, 24, 80).unwrap();
        assert!(buf.is_empty());
    }

    #[test]
    fn test_render_active_pane_with_content() {
        let mut manager = RemotePaneManager::new(80, 24);
        // Create a pane via layout change
        manager.handle_message(protocol::ControlModeMessage::LayoutChange {
            window_id: "@0".into(),
            layout_string: "80x24,0,0,0".into(),
        });
        // Set it as active
        manager.handle_message(protocol::ControlModeMessage::WindowPaneChanged {
            window_id: "@0".into(),
            pane_id: "%0".into(),
        });
        // Feed some output
        manager.handle_message(protocol::ControlModeMessage::Output {
            pane_id: "%0".into(),
            data: b"hello world".to_vec(),
        });

        let mut buf: Vec<u8> = Vec::new();
        render_active_pane(&manager, &mut buf, 24, 80).unwrap();
        let output = String::from_utf8_lossy(&buf);
        // Should contain cursor-home and the text
        assert!(output.contains("\x1b[H"));
        assert!(output.contains("hello world"));
    }
}
