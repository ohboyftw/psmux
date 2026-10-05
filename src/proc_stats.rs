//! Process accounting for machines that leak exited processes.
//!
//! On some Windows machines every process that exits stays resident as a
//! "zombie" (an exited process object a kernel driver still references, ~64 KB
//! each) until reboot. Two read-only signals help there:
//!
//! - spawn counters: how many processes this server has started, by kind, so
//!   psmux's own share of the leak is visible;
//! - a machine-wide zombie count, read in-process. Showing it through a `#()`
//!   status command would itself spawn (and leak) a process every refresh.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// What a spawned process was for.
#[derive(Debug, Clone, Copy)]
pub enum SpawnKind {
    /// A pane or popup shell started through ConPTY.
    Pane,
    /// A `#(command)` substitution in a format string.
    Status,
    /// Everything else: run-shell, hooks, if-shell, copy-pipe, exec, plugin
    /// scripts, toasts, warm-pool servers.
    Other,
}

static PANE: AtomicU64 = AtomicU64::new(0);
static STATUS: AtomicU64 = AtomicU64::new(0);
static OTHER: AtomicU64 = AtomicU64::new(0);

/// Record one process started by this server.
pub fn record_spawn(kind: SpawnKind) {
    let counter = match kind {
        SpawnKind::Pane => &PANE,
        SpawnKind::Status => &STATUS,
        SpawnKind::Other => &OTHER,
    };
    counter.fetch_add(1, Ordering::Relaxed);
}

/// Number of processes of `kind` this server has started since it began.
pub fn spawn_count(kind: SpawnKind) -> u64 {
    match kind {
        SpawnKind::Pane => &PANE,
        SpawnKind::Status => &STATUS,
        SpawnKind::Other => &OTHER,
    }
    .load(Ordering::Relaxed)
}

/// The scan probes hundreds of thousands of PIDs on a leaking machine (seconds
/// of work), so it never runs more often than this, whatever the status interval.
const MIN_SCAN_INTERVAL: Duration = Duration::from_secs(30);

struct ZombieCache {
    /// Zombie count from the last finished scan; `u64::MAX` until one finishes.
    count: AtomicU64,
    scanning: AtomicBool,
    last_scan: std::sync::Mutex<Option<Instant>>,
}

fn zombie_cache() -> &'static ZombieCache {
    static CACHE: OnceLock<ZombieCache> = OnceLock::new();
    CACHE.get_or_init(|| ZombieCache {
        count: AtomicU64::new(u64::MAX),
        scanning: AtomicBool::new(false),
        last_scan: std::sync::Mutex::new(None),
    })
}

/// Machine-wide zombie process count, or `None` before the first scan finishes.
///
/// Never blocks: returns the cached value and, when it is older than
/// `max(refresh, 30s)`, starts a rescan on a background thread.
pub fn zombie_count(refresh: Duration) -> Option<u64> {
    let cache = zombie_cache();
    let stale = match cache.last_scan.lock() {
        Ok(last) => last.is_none_or(|t| t.elapsed() >= refresh.max(MIN_SCAN_INTERVAL)),
        Err(_) => false,
    };
    if stale
        && cache
            .scanning
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        std::thread::spawn(move || {
            if let Some(n) = scan_zombies() {
                cache.count.store(n, Ordering::Relaxed);
            }
            if let Ok(mut last) = cache.last_scan.lock() {
                *last = Some(Instant::now());
            }
            cache.scanning.store(false, Ordering::Release);
        });
    }
    match cache.count.load(Ordering::Relaxed) {
        u64::MAX => None,
        n => Some(n),
    }
}

/// Count exited processes that can still be opened.
///
/// Mirrors `memory-doctor.ps1` exactly (start at PID 8, step 4, stop after
/// 65,536 empty slots in a row) so the number matches the watchdog log.
#[cfg(windows)]
fn scan_zombies() -> Option<u64> {
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
        fn GetExitCodeProcess(process: isize, exit_code: *mut u32) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }

    let mut zombies = 0u64;
    let mut empty_run = 0u32;
    let mut pid = 8u32;
    while empty_run < 65_536 {
        // SAFETY: OpenProcess takes no pointers; a zero return means no handle.
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h == 0 {
            empty_run += 1;
        } else {
            empty_run = 0;
            let mut code = 0u32;
            // SAFETY: `h` is a live handle we own and `code` outlives the call.
            // The handle is closed straight away: holding a handle to an exited
            // process is itself what keeps a zombie alive.
            unsafe {
                if GetExitCodeProcess(h, &mut code) != 0 && code != STILL_ACTIVE {
                    zombies += 1;
                }
                CloseHandle(h);
            }
        }
        pid = pid.checked_add(4)?;
    }
    Some(zombies)
}

#[cfg(not(windows))]
fn scan_zombies() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_spawn_when_called_then_its_kind_count_grows() {
        // Counters are process-global and other tests may spawn panes
        // concurrently, so assert growth rather than an exact value.
        let before = spawn_count(SpawnKind::Status);
        record_spawn(SpawnKind::Status);
        assert!(spawn_count(SpawnKind::Status) > before);
    }

    #[cfg(windows)]
    #[test]
    fn scan_zombies_when_run_then_finishes_with_a_count() {
        assert!(scan_zombies().is_some());
    }
}
