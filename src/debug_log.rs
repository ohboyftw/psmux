//! Centralized debug logging for psmux.
//!
//! All logs write to `~/.psmux/` and are gated by environment variables.
//! Nothing is stored in the repo or source tree — only in the user's
//! home directory under `.psmux/`.
//!
//! ## Environment Variables
//!
//! | Variable               | Log file                          | Description                          |
//! |------------------------|-----------------------------------|--------------------------------------|
//! | `PSMUX_CLIENT_DEBUG=1` | `~/.psmux/client_debug.log`       | Client TUI rendering, draw, status   |
//! | `PSMUX_STYLE_DEBUG=1`  | `~/.psmux/style_debug.log`        | Style/theme parsing, inline styles   |/// | `PSMUX_INPUT_DEBUG=1`  | `~/.psmux/input_debug.log`        | Every crossterm event + console mode |//! | `PSMUX_MOUSE_DEBUG=1`  | `~/.psmux/mouse_debug.log`        | Mouse injection (existing)           |
//! | `PSMUX_SSH_DEBUG=1`    | `~/.psmux/ssh_input.log`          | SSH input handling (existing)        |
//! | `PSMUX_LATENCY_LOG=1`  | `~/.psmux/latency.log`            | Keypress-to-render latency (existing)|
//! | `PSMUX_MEMORY_DEBUG=1` | `~/.psmux/memory_debug.log`       | Frame push, copy mode, scroll, memory|
//!
//! All loggers are:
//! - **Off by default** — zero overhead when disabled (one atomic load per call)
//! - **Capped** — auto-stop after N entries to prevent disk fill
//! - **Thread-safe** — use `LazyLock<Mutex<Option<File>>>`
//! - **Timestamped** — `[HH:MM:SS.mmm]` prefix on every line
//! - **Truncated on startup** — fresh log each session (no stale data)

use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex};

/// Resolve the psmux data directory (`~/.psmux/`).
fn psmux_dir() -> String {
    crate::paths::psmux_dir()
}

/// Open a log file in the psmux data directory, creating the directory if needed.
/// Returns `None` if the file cannot be created.
fn open_log(filename: &str) -> Option<std::fs::File> {
    let dir = psmux_dir();
    let _ = std::fs::create_dir_all(&dir);
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(true) // fresh log each session
        .write(true)
        .open(format!("{}/{}", dir, filename))
        .ok()
}

/// Check if an env var is set to a truthy value ("1" or "true").
fn env_enabled(var: &str) -> bool {
    std::env::var(var).is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

// ─── Client debug log ───────────────────────────────────────────────────────

/// Client debug log file, gated by `PSMUX_CLIENT_DEBUG=1`.
/// Covers: frame receive, JSON parse, draw lifecycle, status bar rendering.
static CLIENT_LOG: LazyLock<Mutex<Option<std::fs::File>>> = LazyLock::new(|| {
    if !env_enabled("PSMUX_CLIENT_DEBUG") {
        return Mutex::new(None);
    }
    Mutex::new(open_log("client_debug.log"))
});

static CLIENT_LOG_COUNT: AtomicU32 = AtomicU32::new(0);

/// Maximum log entries per session to prevent disk fill.
const CLIENT_LOG_CAP: u32 = 5000;

/// Log a client debug message. No-op unless `PSMUX_CLIENT_DEBUG=1`.
///
/// # Arguments
/// * `component` — short tag like `"frame"`, `"draw"`, `"status"`, `"parse"`
/// * `msg` — the log message (should not contain newlines)
pub fn client_log(component: &str, msg: &str) {
    let n = CLIENT_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if n >= CLIENT_LOG_CAP {
        if n == CLIENT_LOG_CAP {
            // Log one final "cap reached" message
            if let Ok(mut guard) = CLIENT_LOG.lock() {
                if let Some(ref mut f) = *guard {
                    let _ = writeln!(f, "[{}][log] --- log cap reached ({} entries), further logging suppressed ---",
                        chrono::Local::now().format("%H:%M:%S%.3f"), CLIENT_LOG_CAP);
                    let _ = f.flush();
                }
            }
        }
        return;
    }
    if let Ok(mut guard) = CLIENT_LOG.lock() {
        if let Some(ref mut f) = *guard {
            let _ = writeln!(
                f,
                "[{}][{}] {}",
                chrono::Local::now().format("%H:%M:%S%.3f"),
                component,
                msg
            );
            let _ = f.flush();
        }
    }
}

/// Returns `true` if client debug logging is active.
pub fn client_log_enabled() -> bool {
    CLIENT_LOG.lock().ok().is_some_and(|g| g.is_some())
}

// ─── Style debug log ────────────────────────────────────────────────────────

/// Style/theme parsing debug log, gated by `PSMUX_STYLE_DEBUG=1`.
/// Covers: inline style parsing, unclosed directives, color mapping.
static STYLE_LOG: LazyLock<Mutex<Option<std::fs::File>>> = LazyLock::new(|| {
    if !env_enabled("PSMUX_STYLE_DEBUG") {
        return Mutex::new(None);
    }
    Mutex::new(open_log("style_debug.log"))
});

static STYLE_LOG_COUNT: AtomicU32 = AtomicU32::new(0);
const STYLE_LOG_CAP: u32 = 2000;

/// Log a style debug message. No-op unless `PSMUX_STYLE_DEBUG=1`.
pub fn style_log(component: &str, msg: &str) {
    let n = STYLE_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if n >= STYLE_LOG_CAP {
        if n == STYLE_LOG_CAP {
            if let Ok(mut guard) = STYLE_LOG.lock() {
                if let Some(ref mut f) = *guard {
                    let _ = writeln!(
                        f,
                        "[{}][log] --- log cap reached ---",
                        chrono::Local::now().format("%H:%M:%S%.3f")
                    );
                    let _ = f.flush();
                }
            }
        }
        return;
    }
    if let Ok(mut guard) = STYLE_LOG.lock() {
        if let Some(ref mut f) = *guard {
            let _ = writeln!(
                f,
                "[{}][{}] {}",
                chrono::Local::now().format("%H:%M:%S%.3f"),
                component,
                msg
            );
            let _ = f.flush();
        }
    }
}

/// Returns `true` if style debug logging is active.
pub fn style_log_enabled() -> bool {
    STYLE_LOG.lock().ok().is_some_and(|g| g.is_some())
}

// ─── Input debug log ────────────────────────────────────────────────────────

/// Input event debug log, gated by `PSMUX_INPUT_DEBUG=1`.
/// Traces every crossterm event + console input mode at startup.
static INPUT_LOG: LazyLock<Mutex<Option<std::fs::File>>> = LazyLock::new(|| {
    if !env_enabled("PSMUX_INPUT_DEBUG") {
        return Mutex::new(None);
    }
    Mutex::new(open_log("input_debug.log"))
});

static INPUT_LOG_COUNT: AtomicU32 = AtomicU32::new(0);
const INPUT_LOG_CAP: u32 = 10000;

/// Log an input debug message. No-op unless `PSMUX_INPUT_DEBUG=1`.
pub fn input_log(component: &str, msg: &str) {
    let n = INPUT_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if n >= INPUT_LOG_CAP {
        if n == INPUT_LOG_CAP {
            if let Ok(mut guard) = INPUT_LOG.lock() {
                if let Some(ref mut f) = *guard {
                    let _ = writeln!(
                        f,
                        "[{}][log] --- log cap reached ---",
                        chrono::Local::now().format("%H:%M:%S%.3f")
                    );
                    let _ = f.flush();
                }
            }
        }
        return;
    }
    if let Ok(mut guard) = INPUT_LOG.lock() {
        if let Some(ref mut f) = *guard {
            let _ = writeln!(
                f,
                "[{}][{}] {}",
                chrono::Local::now().format("%H:%M:%S%.3f"),
                component,
                msg
            );
            let _ = f.flush();
        }
    }
}

/// Returns `true` if input debug logging is active.
pub fn input_log_enabled() -> bool {
    INPUT_LOG.lock().ok().is_some_and(|g| g.is_some())
}

// ─── Server debug log ───────────────────────────────────────────────────────

/// Server debug log, gated by `PSMUX_SERVER_DEBUG=1`.
/// Traces active_idx changes, command dispatch, etc.
static SERVER_LOG: LazyLock<Mutex<Option<std::fs::File>>> = LazyLock::new(|| {
    if !env_enabled("PSMUX_SERVER_DEBUG") {
        return Mutex::new(None);
    }
    Mutex::new(open_log("server_debug.log"))
});

static SERVER_LOG_COUNT: AtomicU32 = AtomicU32::new(0);
const SERVER_LOG_CAP: u32 = 10000;

/// Log a server debug message. No-op unless `PSMUX_SERVER_DEBUG=1`.
pub fn server_log(component: &str, msg: &str) {
    let n = SERVER_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if n >= SERVER_LOG_CAP {
        if n == SERVER_LOG_CAP {
            if let Ok(mut guard) = SERVER_LOG.lock() {
                if let Some(ref mut f) = *guard {
                    let _ = writeln!(
                        f,
                        "[{}][log] --- log cap reached ---",
                        chrono::Local::now().format("%H:%M:%S%.3f")
                    );
                    let _ = f.flush();
                }
            }
        }
        return;
    }
    if let Ok(mut guard) = SERVER_LOG.lock() {
        if let Some(ref mut f) = *guard {
            let _ = writeln!(
                f,
                "[{}][{}] {}",
                chrono::Local::now().format("%H:%M:%S%.3f"),
                component,
                msg
            );
            let _ = f.flush();
        }
    }
}

/// Returns `true` if server debug logging is active.
pub fn server_log_enabled() -> bool {
    SERVER_LOG.lock().ok().is_some_and(|g| g.is_some())
}

// ─── Memory debug log ──────────────────────────────────────────────────────

/// Memory diagnostic log, gated by `PSMUX_MEMORY_DEBUG=1`.
/// Traces: frame push sizes, receiver counts, copy mode transitions,
/// scroll event rates, periodic process memory snapshots.
static MEMORY_LOG: LazyLock<Mutex<Option<std::fs::File>>> = LazyLock::new(|| {
    if !env_enabled("PSMUX_MEMORY_DEBUG") {
        return Mutex::new(None);
    }
    Mutex::new(open_log("memory_debug.log"))
});

static MEMORY_LOG_COUNT: AtomicU32 = AtomicU32::new(0);
/// Higher cap — memory issues can take minutes to manifest.
const MEMORY_LOG_CAP: u32 = 50_000;

/// Log a memory debug message. No-op unless `PSMUX_MEMORY_DEBUG=1`.
pub fn memory_log(component: &str, msg: &str) {
    let n = MEMORY_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if n >= MEMORY_LOG_CAP {
        if n == MEMORY_LOG_CAP {
            if let Ok(mut guard) = MEMORY_LOG.lock() {
                if let Some(ref mut f) = *guard {
                    let _ = writeln!(
                        f,
                        "[{}][log] --- log cap reached ({}) ---",
                        chrono::Local::now().format("%H:%M:%S%.3f"),
                        MEMORY_LOG_CAP
                    );
                    let _ = f.flush();
                }
            }
        }
        return;
    }
    if let Ok(mut guard) = MEMORY_LOG.lock() {
        if let Some(ref mut f) = *guard {
            let _ = writeln!(
                f,
                "[{}][{}] {}",
                chrono::Local::now().format("%H:%M:%S%.3f"),
                component,
                msg
            );
            let _ = f.flush();
        }
    }
}

/// Returns `true` if memory debug logging is active.
pub fn memory_log_enabled() -> bool {
    MEMORY_LOG.lock().ok().is_some_and(|g| g.is_some())
}

/// Query the current process's working set size (bytes) via Win32 API.
/// Returns 0 on failure.
pub fn process_memory_bytes() -> u64 {
    #[cfg(windows)]
    {
        use std::mem::MaybeUninit;
        // PROCESS_MEMORY_COUNTERS_EX — we only need WorkingSetSize and
        // PrivateUsage, but must pass the full struct.
        #[repr(C)]
        #[allow(non_snake_case)]
        struct PROCESS_MEMORY_COUNTERS {
            cb: u32,
            PageFaultCount: u32,
            PeakWorkingSetSize: usize,
            WorkingSetSize: usize,
            QuotaPeakPagedPoolUsage: usize,
            QuotaPagedPoolUsage: usize,
            QuotaPeakNonPagedPoolUsage: usize,
            QuotaNonPagedPoolUsage: usize,
            PagefileUsage: usize,
            PeakPagefileUsage: usize,
        }
        extern "system" {
            fn K32GetProcessMemoryInfo(
                process: isize,
                ppsmemcounters: *mut PROCESS_MEMORY_COUNTERS,
                cb: u32,
            ) -> i32;
            fn GetCurrentProcess() -> isize;
        }
        unsafe {
            let mut pmc = MaybeUninit::<PROCESS_MEMORY_COUNTERS>::zeroed().assume_init();
            pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) != 0 {
                return pmc.WorkingSetSize as u64;
            }
        }
        0
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Format bytes as a human-readable string (KB/MB/GB).
pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{} KB", bytes / 1024)
    }
}
