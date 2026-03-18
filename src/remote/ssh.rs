//! SSH transport for connecting to remote tmux in control mode.
//!
//! [`SshTransport`] wraps an SSH child process whose stdin/stdout carry the
//! tmux `-CC` control mode protocol. Commands are sent as plain text lines
//! and responses are read line-by-line for feeding into
//! [`super::parser::ControlModeParser`].

use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

/// Transport layer for SSH connection to remote tmux in control mode.
pub struct SshTransport {
    child: Child,
    stdin: Box<dyn Write + Send>,
    reader: BufReader<Box<dyn io::Read + Send>>,
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
        let stdin = Box::new(child.stdin.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "Failed to capture SSH stdin")
        })?);
        let stdout: Box<dyn io::Read + Send> = Box::new(child.stdout.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "Failed to capture SSH stdout")
        })?);
        let reader = BufReader::new(stdout);

        Ok(Self {
            child,
            stdin,
            reader,
        })
    }

    /// Send a tmux command to the remote session.
    ///
    /// In control mode, commands are sent as plain text lines terminated by a
    /// newline.
    pub fn send_command(&mut self, cmd: &str) -> io::Result<()> {
        writeln!(self.stdin, "{}", cmd)?;
        self.stdin.flush()
    }

    /// Read a single line from the SSH stdout (control mode output).
    pub fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        self.reader.read_line(buf)
    }

    /// Check if the SSH process is still running.
    pub fn is_alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }

    /// Gracefully disconnect from the remote tmux session.
    ///
    /// Sends a `detach` command and then waits for the child process to exit.
    pub fn disconnect(mut self) {
        // Send detach command
        let _ = self.send_command("detach");
        // Wait for the child to exit
        let _ = self.child.wait();
    }
}

impl Drop for SshTransport {
    fn drop(&mut self) {
        // Ensure child process is cleaned up even if disconnect() was not called.
        let _ = self.child.kill();
        let _ = self.child.wait();
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
