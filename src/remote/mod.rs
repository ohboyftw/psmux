pub mod octal;
pub mod protocol;
pub mod parser;
pub mod pane_manager;
pub mod ssh;

use parser::ControlModeParser;
use pane_manager::RemotePaneManager;
use ssh::SshTransport;

/// Main entry point for remote tmux session rendering.
///
/// Connects to a remote host via SSH, starts (or attaches to) a tmux session
/// in control mode (`-CC`), and feeds the control mode event stream through
/// the parser and pane manager pipeline.
///
/// This is a minimal non-interactive version that validates the end-to-end
/// pipeline.  Task 16 will add the full ratatui rendering loop.
pub fn run_remote_tmux(
    ssh_target: &str,
    session: Option<&str>,
    ssh_opts: Option<&str>,
) -> std::io::Result<()> {
    let session_name = session.unwrap_or("default");

    // Try attach first, fall back to new-session
    let mut transport = SshTransport::connect(ssh_target, session_name, true, ssh_opts)
        .or_else(|_| SshTransport::connect(ssh_target, session_name, false, ssh_opts))?;

    let (cols, rows) = crossterm::terminal::size()?;
    transport.send_command(&format!("refresh-client -C {}x{}", cols, rows))?;

    let mut cm_parser = ControlModeParser::new();
    let mut manager = RemotePaneManager::new(cols, rows);

    // Read loop — feed lines from SSH stdout to parser
    let mut line = String::new();
    loop {
        line.clear();
        match transport.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {
                if let Some(msg) = cm_parser.feed_line(&line) {
                    if matches!(msg, protocol::ControlModeMessage::Exit { .. }) {
                        break;
                    }
                    manager.handle_message(msg);
                }
            }
            Err(e) => {
                eprintln!("SSH read error: {}", e);
                break;
            }
        }
    }

    transport.disconnect();
    Ok(())
}
