//! SSH transport for connecting to remote tmux in control mode.
//!
//! [`SshTransport`] wraps an SSH child process whose stdin/stdout carry the
//! tmux `-CC` control mode protocol. Commands are sent as plain text lines
//! and responses are read line-by-line for feeding into
//! [`super::parser::ControlModeParser`].
//!
//! The transport can be split into an [`SshReader`] (moved to a background
//! thread) and an [`SshWriter`] (kept on the main thread) via
//! [`SshTransport::split`].

use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

/// Transport layer for SSH connection to remote tmux in control mode.
///
/// Fields are wrapped in `Option` so that [`split`](Self::split) can move
/// them out without conflicting with the `Drop` implementation.
pub struct SshTransport {
    child: Option<Child>,
    stdin: Option<Box<dyn Write + Send>>,
    reader: Option<BufReader<Box<dyn io::Read + Send>>>,
}

/// Read half of a split [`SshTransport`].
///
/// Owns the buffered reader over SSH stdout. Designed to be moved to a
/// background thread that feeds lines into a channel.
pub struct SshReader {
    reader: BufReader<Box<dyn io::Read + Send>>,
}

impl SshReader {
    /// Read a single line from the SSH stdout (control mode output).
    ///
    /// Returns the number of bytes read (0 = EOF).
    pub fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        self.reader.read_line(buf)
    }
}

/// Write half of a split [`SshTransport`].
///
/// Owns SSH stdin and the child process handle. Stays on the main thread
/// to send tmux commands and manage the process lifecycle.
pub struct SshWriter {
    child: Child,
    stdin: Box<dyn Write + Send>,
}

impl SshWriter {
    /// Send a tmux command to the remote session.
    pub fn send_command(&mut self, cmd: &str) -> io::Result<()> {
        writeln!(self.stdin, "{}", cmd)?;
        self.stdin.flush()
    }

    /// Gracefully disconnect from the remote tmux session.
    ///
    /// Sends a `detach` command and then waits for the child process to exit.
    pub fn disconnect(mut self) {
        let _ = self.send_command("detach");
        let _ = self.child.wait();
    }
}

impl Drop for SshWriter {
    fn drop(&mut self) {
        // Ensure child process is cleaned up even if disconnect() was not called.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl SshTransport {
    /// Connect to a remote host and start tmux in control mode (`-CC`).
    ///
    /// If `attach` is true, attaches to an existing session; otherwise creates
    /// a new one. Optional `ssh_opts` are split on whitespace and passed as
    /// additional arguments to the `ssh` command.
    pub fn connect(
        ssh_target: &str,
        session: &str,
        attach: bool,
        ssh_opts: Option<&str>,
    ) -> io::Result<Self> {
        let tmux_cmd = if attach {
            format!("tmux -CC attach -t {}", session)
        } else {
            format!("tmux -CC new-session -s {}", session)
        };

        let mut cmd = Command::new("ssh");
        if let Some(opts) = ssh_opts {
            for opt in opts.split_whitespace() {
                cmd.arg(opt);
            }
        }
        cmd.arg(ssh_target)
            .arg(&tmux_cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdin = Box::new(
            child
                .stdin
                .take()
                .ok_or_else(|| io::Error::other("Failed to capture SSH stdin"))?,
        );
        let stdout: Box<dyn io::Read + Send> = Box::new(
            child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("Failed to capture SSH stdout"))?,
        );
        let reader = BufReader::new(stdout);

        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            reader: Some(reader),
        })
    }

    /// Send a tmux command to the remote session.
    ///
    /// In control mode, commands are sent as plain text lines terminated by a
    /// newline.
    pub fn send_command(&mut self, cmd: &str) -> io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "transport already split"))?;
        writeln!(stdin, "{}", cmd)?;
        stdin.flush()
    }

    /// Read a single line from the SSH stdout (control mode output).
    pub fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        let reader = self
            .reader
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "transport already split"))?;
        reader.read_line(buf)
    }

    /// Check if the SSH process is still running.
    pub fn is_alive(&mut self) -> bool {
        self.child
            .as_mut()
            .and_then(|c| c.try_wait().ok().flatten())
            .is_none()
    }

    /// Split the transport into separate reader and writer halves.
    ///
    /// This is used by the interactive rendering loop: the [`SshReader`] is
    /// moved to a background thread that feeds lines into a channel, while
    /// the [`SshWriter`] stays on the main thread to send tmux commands and
    /// receive keyboard input.
    ///
    /// After calling `split`, the original `SshTransport` is left in a
    /// hollow state — its `Drop` impl becomes a no-op since the child
    /// process is now owned by `SshWriter`.
    pub fn split(mut self) -> (SshReader, SshWriter) {
        let reader = self
            .reader
            .take()
            .expect("split() called on already-split transport");
        let stdin = self
            .stdin
            .take()
            .expect("split() called on already-split transport");
        let child = self
            .child
            .take()
            .expect("split() called on already-split transport");

        (SshReader { reader }, SshWriter { child, stdin })
    }

    /// Gracefully disconnect from the remote tmux session.
    ///
    /// Sends a `detach` command and then waits for the child process to exit.
    pub fn disconnect(mut self) {
        // Send detach command
        let _ = self.send_command("detach");
        // Wait for the child to exit
        if let Some(ref mut child) = self.child {
            let _ = child.wait();
        }
    }
}

impl Drop for SshTransport {
    fn drop(&mut self) {
        // Ensure child process is cleaned up even if disconnect() was not called.
        // After split(), child is None so this is a no-op.
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_transport_creation_fails_without_ssh() {
        // This test verifies error handling when SSH isn't available
        // or the target doesn't exist. We use an unreachable host with a
        // short timeout so the test doesn't hang.
        let result = SshTransport::connect(
            "nonexistent.invalid.host.example.com",
            "test",
            false,
            Some("-o ConnectTimeout=1 -o StrictHostKeyChecking=no"),
        );
        // Either spawn fails (no ssh binary) or we get an SshTransport that
        // will fail on read. Both are acceptable — we're testing the API
        // doesn't panic.
        let _ = result;
    }
}
