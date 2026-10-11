//! kill-server and the warm standby.
//!
//! A server spawns its namespace's `__warm__` standby a moment AFTER it has
//! registered, and the standby needs a few hundred milliseconds more before its
//! own `.port` appears. A kill-server that lands in that window enumerates the
//! registry, finds the session server but not the standby, ends the session
//! server and returns; the standby registers afterwards and nothing ever ends
//! it (upstream measured 10 leaks out of 10 for `new-session -d` followed at
//! once by `kill-server`).
//!
//! No amount of waiting in kill-server closes that, because the standby can
//! register at any later moment. So the kill leaves a marker instead, and the
//! standby checks it: kill-server stamps the time into a per scope marker file
//! before it enumerates and again after, and an unclaimed standby that finds a
//! marker covering it ends itself. See [`warm_standby_verdict`] for why the two
//! stamps make the ordering airtight.
//!
//! Times are FILETIMEs (100 ns ticks since 1601-01-01 UTC), the unit Windows
//! reports process creation times in, so a marker and a creation time compare
//! directly.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How often an unclaimed standby asks whether it has been orphaned.
pub const WARM_ORPHAN_CHECK_INTERVAL: Duration = Duration::from_millis(250);

/// FILETIME ticks between 1601-01-01 and 1970-01-01.
const FILETIME_UNIX_EPOCH: u64 = 116_444_736_000_000_000;

/// The current wall-clock time as a FILETIME.
pub fn now_filetime() -> u64 {
    let since_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    FILETIME_UNIX_EPOCH + (since_unix.as_nanos() / 100) as u64
}

/// Which servers a kill-server reached: every namespace (`kill-server` with no
/// `-L`, which ends all servers) or one `-L` namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillScope<'a> {
    /// Every server, in every namespace.
    All,
    /// The servers of one `-L` namespace.
    Namespace(&'a str),
}

/// The kill marker for `scope`, inside `dir/killed/`. Namespace markers carry an
/// `ns-` prefix so that a namespace literally named `all` cannot collide with
/// the everything marker. `-L` names are already used verbatim as `.port` file
/// name prefixes, so they are legal file name characters here too.
pub fn kill_marker_path(dir: &Path, scope: KillScope) -> PathBuf {
    let killed = dir.join("killed");
    match scope {
        KillScope::All => killed.join("all"),
        KillScope::Namespace(ns) => killed.join(format!("ns-{ns}")),
    }
}

/// Stamp `when` (a FILETIME) into the kill marker for `scope`. Written to a
/// temporary file and renamed over the marker, so a standby reading it at the
/// same moment sees the old stamp or the new one, never an empty file.
pub fn write_kill_marker(dir: &Path, scope: KillScope, when: u64) {
    let path = kill_marker_path(dir, scope);
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&tmp, when.to_string()).is_ok() && std::fs::rename(&tmp, &path).is_ok() {
        return;
    }
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::write(&path, when.to_string());
}

/// The newest kill that covers namespace `ns` (`None` = the default
/// namespace): its own marker or the everything marker, whichever is later.
/// The default namespace is reached only by an everything kill.
pub fn read_kill_marker(dir: &Path, ns: Option<&str>) -> Option<u64> {
    let read = |scope: KillScope| -> Option<u64> {
        std::fs::read_to_string(kill_marker_path(dir, scope))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
    };
    let own = ns.and_then(|n| read(KillScope::Namespace(n)));
    match (own, read(KillScope::All)) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

/// Render a process identity as `pid:creation`, the `--spawner` argument.
pub fn format_spawner(pid: u32, creation: u64) -> String {
    format!("{pid}:{creation}")
}

/// Parse a `pid:creation` spawner argument. Zero in either field is rejected:
/// neither names a real process.
pub fn parse_spawner(spec: &str) -> Option<(u32, u64)> {
    let (pid, creation) = spec.trim().split_once(':')?;
    let pid = pid.parse::<u32>().ok().filter(|p| *p != 0)?;
    let creation = creation.parse::<u64>().ok().filter(|c| *c != 0)?;
    Some((pid, creation))
}

/// The server that spawned this standby, from the `--spawner` argument. Unset
/// for a standby started any other way and for every non standby.
static WARM_SPAWNER: OnceLock<(u32, u64)> = OnceLock::new();

/// Record the spawner named by `--spawner pid:creation`. Called once, from the
/// `server` argument parser, before `run_server`.
pub fn set_warm_spawner(spec: &str) {
    if let Some(identity) = parse_spawner(spec) {
        let _ = WARM_SPAWNER.set(identity);
    }
}

/// The spawner recorded by [`set_warm_spawner`], if any.
pub fn warm_spawner() -> Option<(u32, u64)> {
    WARM_SPAWNER.get().copied()
}

/// What an unclaimed standby knows about itself when it asks whether it should
/// still be running. Times are FILETIMEs; `None` means unknown, and an unknown
/// value never makes a standby exit.
#[derive(Debug, Clone, Copy)]
pub struct WarmStandbyFacts {
    /// When this standby process was created.
    pub own_creation: Option<u64>,
    /// When the server that spawned this standby was created, if it said.
    pub spawner_creation: Option<u64>,
    /// Whether that spawner is still running under the same identity.
    pub spawner_alive: bool,
    /// The newest kill marker covering this namespace.
    pub kill_marker: Option<u64>,
    /// Whether the data directory still exists.
    pub data_dir_present: bool,
}

/// Why an unclaimed standby must end, or `Keep`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarmStandbyVerdict {
    /// Nothing says this standby is orphaned.
    Keep,
    /// The data directory was deleted. A standby serves the sessions of one
    /// data directory; recreating it resurrects a namespace somebody removed on
    /// purpose.
    DataDirGone,
    /// A kill-server covering this namespace ran after this standby was created.
    NamespaceKilled,
    /// The spawner is dead and a kill-server ran after the spawner was
    /// created: the kill that ended the spawner is the one this standby missed.
    SpawnerKilled,
}

/// The decision, kept free of I/O so it can be tested exhaustively.
///
/// Why this closes the race rather than narrowing it. kill-server writes stamp
/// M1, enumerates the registry, writes stamp M2, then ends what it found.
///
/// * A standby that already existed at M1 either registered before the
///   enumeration, and was ended by it, or registered after it. In the second
///   case its first check comes after its own registration, so after M1 was
///   written: it reads a stamp at or after M1, which is at or after its own
///   creation, and exits (`NamespaceKilled`).
/// * A standby created after M1 was spawned by some server X. If X was ended by
///   this kill, X was in the enumeration, so X was created before M2 was
///   written, and X died after M2 was written. The standby sees X gone, reads a
///   stamp at or after M2, which is after X's creation, and exits
///   (`SpawnerKilled`). If X survived (it registered too late to be
///   enumerated) the standby survives with it, which is right for a server
///   that is still running.
pub fn warm_standby_verdict(f: &WarmStandbyFacts) -> WarmStandbyVerdict {
    if !f.data_dir_present {
        return WarmStandbyVerdict::DataDirGone;
    }
    let Some(marker) = f.kill_marker else {
        return WarmStandbyVerdict::Keep;
    };
    if f.own_creation.is_some_and(|own| marker >= own) {
        return WarmStandbyVerdict::NamespaceKilled;
    }
    if !f.spawner_alive && f.spawner_creation.is_some_and(|sp| marker >= sp) {
        return WarmStandbyVerdict::SpawnerKilled;
    }
    WarmStandbyVerdict::Keep
}

/// Gather the facts for the standby running in this process, in namespace
/// `ns`, against data directory `dir`, and judge them.
pub fn check_this_standby(dir: &Path, ns: Option<&str>) -> WarmStandbyVerdict {
    let spawner = warm_spawner();
    let facts = WarmStandbyFacts {
        own_creation: process_creation_time(std::process::id()),
        spawner_creation: spawner.map(|(_, c)| c),
        // Creation time alone is not liveness (an exited process whose object
        // is still held open answers GetProcessTimes), hence both checks.
        spawner_alive: spawner
            .is_some_and(|(pid, c)| process_is_alive(pid) && process_creation_time(pid) == Some(c)),
        kill_marker: read_kill_marker(dir, ns),
        data_dir_present: dir.is_dir(),
    };
    warm_standby_verdict(&facts)
}

#[cfg(windows)]
mod ffi {
    pub const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    pub const STILL_ACTIVE: u32 = 259;

    #[repr(C)]
    #[derive(Default)]
    pub struct FileTime {
        pub low: u32,
        pub high: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
        pub fn CloseHandle(handle: isize) -> i32;
        pub fn GetExitCodeProcess(h_process: isize, exit_code: *mut u32) -> i32;
        pub fn GetProcessTimes(
            h_process: isize,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }

    /// Run `f` with a query handle on `pid`, closing it afterwards.
    pub fn with_process<T>(pid: u32, f: impl FnOnce(isize) -> Option<T>) -> Option<T> {
        // SAFETY: OpenProcess has no memory preconditions; a 0/-1 return is
        // checked before the handle is used.
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h == 0 || h == -1 {
            return None;
        }
        let out = f(h);
        // SAFETY: `h` is a valid handle opened above and closed exactly once.
        unsafe { CloseHandle(h) };
        out
    }
}

/// When process `pid` was created, as a FILETIME. `None` when it cannot be
/// opened (gone, or no access).
#[cfg(windows)]
pub fn process_creation_time(pid: u32) -> Option<u64> {
    ffi::with_process(pid, |h| {
        let mut times: [ffi::FileTime; 4] = Default::default();
        let [c, e, k, u] = &mut times;
        // SAFETY: `h` is a live query handle; the four out-pointers refer to
        // distinct, properly aligned FileTime values that outlive the call.
        let ok = unsafe { ffi::GetProcessTimes(h, c, e, k, u) };
        (ok != 0).then(|| (u64::from(c.high) << 32) | u64::from(c.low))
    })
}

/// Whether process `pid` is still running (has not exited).
#[cfg(windows)]
pub fn process_is_alive(pid: u32) -> bool {
    ffi::with_process(pid, |h| {
        let mut code: u32 = 0;
        // SAFETY: `h` is a live query handle and `code` a valid out-pointer.
        let ok = unsafe { ffi::GetExitCodeProcess(h, &mut code) };
        (ok != 0).then_some(code == ffi::STILL_ACTIVE)
    })
    .unwrap_or(false)
}

/// Creation times are not available off Windows; an unknown never makes a
/// standby exit.
#[cfg(not(windows))]
pub fn process_creation_time(_pid: u32) -> Option<u64> {
    None
}

/// Liveness is not tracked off Windows.
#[cfg(not(windows))]
pub fn process_is_alive(_pid: u32) -> bool {
    false
}

#[cfg(test)]
#[path = "../tests-rs/test_kill_server_reaps_warm.rs"]
mod tests_kill_server_reaps_warm;
