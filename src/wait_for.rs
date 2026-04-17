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

/// Wait for a file to appear at `path`. Checks if already exists first.
///
/// Polls `path.exists()` every 50 ms. Polling is used (rather than
/// `ReadDirectoryChangesW`) because it is simpler, cross-volume safe,
/// and handles parent-directory-created-late cases uniformly. A 50 ms
/// cadence is responsive enough for sentinel-file workflows.
pub fn wait_file(path: &std::path::Path, timeout_ms: u64) -> WaitOutcome {
    const POLL_INTERVAL_MS: u64 = 50;

    let start = Instant::now();
    if path.exists() {
        return WaitOutcome::Success {
            elapsed_ms: start.elapsed().as_millis() as u64,
        };
    }

    let deadline = start + std::time::Duration::from_millis(timeout_ms);
    loop {
        let now = Instant::now();
        if now >= deadline {
            return WaitOutcome::Timeout {
                elapsed_ms: start.elapsed().as_millis() as u64,
            };
        }
        let remaining = deadline - now;
        let sleep_for = remaining.min(std::time::Duration::from_millis(POLL_INTERVAL_MS));
        std::thread::sleep(sleep_for);
        if path.exists() {
            return WaitOutcome::Success {
                elapsed_ms: start.elapsed().as_millis() as u64,
            };
        }
    }
}

/// Wait for a regex pattern to match the pane's live screen buffer.
///
/// `screen_fn` is invoked every `poll_interval_ms` to fetch the current
/// screen text. In production this closure calls `pane.screen().contents()`.
pub fn wait_output<F>(
    pattern: &regex::Regex,
    screen_fn: F,
    timeout_ms: u64,
    poll_interval_ms: u64,
) -> WaitOutcome
where
    F: Fn() -> String,
{
    let start = Instant::now();
    let deadline = start + std::time::Duration::from_millis(timeout_ms);
    let interval = std::time::Duration::from_millis(poll_interval_ms);

    loop {
        let text = screen_fn();
        if pattern.is_match(&text) {
            return WaitOutcome::Success {
                elapsed_ms: start.elapsed().as_millis() as u64,
            };
        }
        let now = Instant::now();
        if now >= deadline {
            return WaitOutcome::Timeout {
                elapsed_ms: start.elapsed().as_millis() as u64,
            };
        }
        let remaining = deadline - now;
        std::thread::sleep(remaining.min(interval));
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

    #[test]
    fn wait_file_returns_success_when_file_preexists() {
        let dir = std::env::temp_dir().join("psmux-test-waitfile");
        std::fs::create_dir_all(&dir).ok();
        let target = dir.join("preexist.flag");
        std::fs::write(&target, "ok").unwrap();
        let result = wait_file(&target, 100);
        assert!(matches!(result, WaitOutcome::Success { .. }));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn wait_file_returns_success_when_file_created_async() {
        let dir = std::env::temp_dir().join("psmux-test-waitfile-async");
        std::fs::create_dir_all(&dir).ok();
        let target = dir.join("delayed.flag");
        let _ = std::fs::remove_file(&target);
        let target2 = target.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            std::fs::write(&target2, "ok").unwrap();
        });
        let result = wait_file(&target, 5000);
        assert!(matches!(result, WaitOutcome::Success { .. }));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn wait_file_returns_timeout_when_file_never_created() {
        let target = std::env::temp_dir()
            .join("psmux-test-waitfile-never")
            .join("nope.flag");
        std::fs::create_dir_all(target.parent().unwrap()).ok();
        let result = wait_file(&target, 200);
        assert!(matches!(result, WaitOutcome::Timeout { .. }));
        std::fs::remove_dir_all(target.parent().unwrap()).ok();
    }

    #[test]
    fn wait_output_matches_immediately() {
        let re = regex::Regex::new("READY").unwrap();
        let result = wait_output(&re, || "some text READY here".to_string(), 1000, 10);
        assert!(matches!(result, WaitOutcome::Success { .. }));
        if let WaitOutcome::Success { elapsed_ms } = result {
            assert!(elapsed_ms < 100, "should match on first poll");
        }
    }

    #[test]
    fn wait_output_matches_after_delay() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let counter = std::sync::Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        let re = regex::Regex::new("DONE$").unwrap();
        let result = wait_output(
            &re,
            move || {
                let n = c.fetch_add(1, Ordering::Relaxed);
                if n >= 5 {
                    "status: DONE".to_string()
                } else {
                    "status: working".to_string()
                }
            },
            5000,
            10,
        );
        assert!(matches!(result, WaitOutcome::Success { .. }));
    }

    #[test]
    fn wait_output_times_out_when_no_match() {
        let re = regex::Regex::new("NEVER_MATCH_THIS").unwrap();
        let result = wait_output(&re, || "hello world".to_string(), 200, 50);
        assert!(matches!(result, WaitOutcome::Timeout { .. }));
    }

    #[test]
    fn wait_output_multiline_regex() {
        let re = regex::Regex::new(r"(?m)^\$ $").unwrap();
        let result = wait_output(&re, || "output line\n$ \n".to_string(), 1000, 10);
        assert!(matches!(result, WaitOutcome::Success { .. }));
    }
}
