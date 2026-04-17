//! Server-side blocking wait for pane conditions.
//!
//! Replaces all client-side polling loops (sentinel files, `#{pane_ready}`,
//! exit code checks) with a single `psmux wait-for` command that blocks
//! until the condition is met or the timeout expires.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// A condition to wait for on a pane.
#[derive(Debug, Clone)]
pub enum WaitCondition {
    /// Wait for a process to exit (by PID).
    Exit { pid: u32 },
    /// Wait for a file to appear at the given path.
    File { path: PathBuf },
    /// Wait for a regex pattern to match the pane's live screen buffer.
    Output { pattern: regex::Regex },
    /// Wait for the pane to reach idle prompt (reuses `context_ready` signal).
    Ready,
}

/// The result of a `wait-for` operation.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WaitOutcome {
    /// The condition was met.
    Success { elapsed_ms: u64 },
    /// The condition was met and the process exited with a code.
    ExitSuccess { elapsed_ms: u64, exit_code: i32 },
    /// The timeout expired before the condition was met.
    Timeout { elapsed_ms: u64 },
    /// An error occurred while waiting.
    Error { reason: String },
}

impl WaitCondition {
    /// Parse a wait condition from CLI/RPC arguments.
    ///
    /// - `kind`: one of `"exit"`, `"file"`, `"output"`, `"ready"`
    /// - `arg`: the kind-specific argument (PID, path, or regex pattern)
    /// - `_opts`: reserved for future per-kind options
    pub fn parse(kind: &str, arg: Option<&str>, _opts: Option<&str>) -> Result<Self, String> {
        match kind {
            "exit" => {
                let pid_str = arg.ok_or("exit requires a PID argument")?;
                let pid: u32 = pid_str
                    .parse()
                    .map_err(|_| format!("invalid PID: {pid_str}"))?;
                Ok(Self::Exit { pid })
            }
            "file" => {
                let path = arg.ok_or("file requires a path argument")?;
                Ok(Self::File {
                    path: PathBuf::from(path),
                })
            }
            "output" => {
                let pat = arg.ok_or("output requires a regex pattern argument")?;
                let re = regex::Regex::new(pat).map_err(|e| format!("invalid regex: {e}"))?;
                Ok(Self::Output { pattern: re })
            }
            "ready" => Ok(Self::Ready),
            other => Err(format!("unknown wait condition: {other}")),
        }
    }
}

impl fmt::Display for WaitCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exit { pid } => write!(f, "exit(pid={pid})"),
            Self::File { path } => write!(f, "file({})", path.display()),
            Self::Output { pattern } => write!(f, "output(/{}/)", pattern.as_str()),
            Self::Ready => write!(f, "ready"),
        }
    }
}
