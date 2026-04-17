//! Server-side blocking wait for pane conditions.
//!
//! Replaces all client-side polling loops (sentinel files, `#{pane_ready}`,
//! exit code checks) with a single `psmux wait-for` command that blocks
//! until the condition is met or the timeout expires.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::time::Instant;

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

// ─── Executors ──────────────────────────────────────────────────────────────

/// Wait for a process to exit by PID. Returns exit code on success.
///
/// Uses `OpenProcess` + `WaitForSingleObject` + `GetExitCodeProcess`.
/// Returns `Error` if the PID cannot be opened (already gone, access denied).
#[cfg(windows)]
pub fn wait_exit(pid: u32, timeout_ms: u64) -> WaitOutcome {
    const PROCESS_SYNCHRONIZE: u32 = 0x0010_0000;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;
    const WAIT_FAILED: u32 = 0xFFFF_FFFF;
    const STILL_ACTIVE: u32 = 259;

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
        fn WaitForSingleObject(handle: isize, milliseconds: u32) -> u32;
        fn GetExitCodeProcess(handle: isize, exit_code: *mut u32) -> i32;
        fn CloseHandle(handle: isize) -> i32;
        fn GetLastError() -> u32;
    }

    struct HandleGuard(isize);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            if self.0 != 0 {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    let start = Instant::now();
    let handle = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        )
    };
    if handle == 0 {
        let err = unsafe { GetLastError() };
        return WaitOutcome::Error {
            reason: format!("OpenProcess({pid}) failed (error {err})"),
        };
    }
    let _guard = HandleGuard(handle);

    let ms = u32::try_from(timeout_ms).unwrap_or(u32::MAX);
    let wait_result = unsafe { WaitForSingleObject(handle, ms) };
    let elapsed_ms = start.elapsed().as_millis() as u64;

    match wait_result {
        WAIT_OBJECT_0 => {
            let mut code: u32 = 0;
            let ok = unsafe { GetExitCodeProcess(handle, &mut code) };
            if ok == 0 {
                let err = unsafe { GetLastError() };
                return WaitOutcome::Error {
                    reason: format!("GetExitCodeProcess failed (error {err})"),
                };
            }
            if code == STILL_ACTIVE {
                return WaitOutcome::Error {
                    reason: "process signaled but STILL_ACTIVE reported".into(),
                };
            }
            WaitOutcome::ExitSuccess {
                elapsed_ms,
                exit_code: code as i32,
            }
        }
        WAIT_TIMEOUT => WaitOutcome::Timeout { elapsed_ms },
        WAIT_FAILED => {
            let err = unsafe { GetLastError() };
            WaitOutcome::Error {
                reason: format!("WaitForSingleObject failed (error {err})"),
            }
        }
        other => WaitOutcome::Error {
            reason: format!("WaitForSingleObject returned unexpected value {other}"),
        },
    }
}

#[cfg(not(windows))]
pub fn wait_exit(_pid: u32, _timeout_ms: u64) -> WaitOutcome {
    WaitOutcome::Error {
        reason: "wait_exit is only implemented on Windows".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn wait_exit_returns_error_for_nonexistent_pid() {
        let result = wait_exit(999_999_999, 100);
        assert!(matches!(result, WaitOutcome::Error { .. }));
    }

    #[test]
    #[cfg(windows)]
    fn wait_exit_returns_success_for_self_spawned_process() {
        let child = std::process::Command::new("cmd")
            .args(["/C", "exit 42"])
            .spawn()
            .unwrap();
        let pid = child.id();
        let result = wait_exit(pid, 5000);
        match result {
            WaitOutcome::ExitSuccess { exit_code, .. } => assert_eq!(exit_code, 42),
            other => panic!("expected ExitSuccess, got {other:?}"),
        }
    }
}
