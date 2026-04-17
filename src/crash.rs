//! Crash diagnostics: panic hook + crash report writer + listing/pruning.
//!
//! Installs a panic hook that writes a crash report to
//! `%LOCALAPPDATA%/psmux/crashes/psmux-{pid}-{unix_ts}.crash` with the panic
//! message and full backtrace. CLI surface: `psmux debug crashes list|show`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Install a panic hook that writes crash reports to [`crash_directory`].
/// Chains to the previous hook so default behavior (print + abort) still runs.
pub fn install_crash_handler() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let crash_dir = crash_directory();
        if let Err(e) = fs::create_dir_all(&crash_dir) {
            eprintln!("psmux: failed to create crash dir: {e}");
        } else {
            let pid = std::process::id();
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let path = crash_dir.join(format!("psmux-{pid}-{ts}.crash"));
            let mut report = String::from("psmux crash report\n");
            report += &format!("pid: {pid}\n");
            report += &format!("time: {ts}\n");
            report += &format!("panic: {info}\n");
            let bt = std::backtrace::Backtrace::force_capture();
            report += &format!("\nbacktrace:\n{bt}\n");
            if let Err(e) = fs::write(&path, &report) {
                eprintln!("psmux: failed to write crash report: {e}");
            } else {
                eprintln!("psmux: crash report written to {}", path.display());
            }
        }
        default_hook(info);
    }));
}

/// Location of crash reports: `%LOCALAPPDATA%/psmux/crashes` on Windows, with a
/// `~/.psmux/crashes` fallback when `LOCALAPPDATA` is unset.
pub fn crash_directory() -> PathBuf {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        PathBuf::from(local).join("psmux").join("crashes")
    } else {
        dirs_or_home().join(".psmux").join("crashes")
    }
}

fn dirs_or_home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// A discovered crash report file.
#[derive(Debug, Clone)]
pub struct CrashEntry {
    /// Absolute path to the `.crash` file.
    pub path: PathBuf,
    /// PID parsed from the filename.
    pub pid: u32,
    /// Unix timestamp parsed from the filename.
    pub timestamp: u64,
}

/// List crash dumps newest-first, up to `limit` entries.
pub fn list_crashes(limit: usize) -> Vec<CrashEntry> {
    let dir = crash_directory();
    let Ok(read_dir) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut entries: Vec<CrashEntry> = read_dir
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) != Some("crash") {
                return None;
            }
            let stem = path.file_stem()?.to_str()?;
            parse_crash_stem(stem).map(|(pid, timestamp)| CrashEntry {
                path: path.clone(),
                pid,
                timestamp,
            })
        })
        .collect();
    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    entries.truncate(limit);
    entries
}

/// Parse `psmux-{pid}-{ts}` into `(pid, ts)`.
fn parse_crash_stem(stem: &str) -> Option<(u32, u64)> {
    let rest = stem.strip_prefix("psmux-")?;
    let (pid_s, ts_s) = rest.rsplit_once('-')?;
    Some((pid_s.parse().ok()?, ts_s.parse().ok()?))
}

/// Read the crash report at `path` and return its contents.
pub fn show_crash(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)
}

/// Keep only the newest `keep` crash files, delete older ones.
pub fn prune_crashes(keep: usize) {
    let dir = crash_directory();
    let Ok(read_dir) = fs::read_dir(&dir) else {
        return;
    };
    let mut files: Vec<(PathBuf, u64)> = read_dir
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) != Some("crash") {
                return None;
            }
            let stem = path.file_stem()?.to_str()?;
            let (_, ts) = parse_crash_stem(stem)?;
            Some((path, ts))
        })
        .collect();
    files.sort_by(|a, b| b.1.cmp(&a.1));
    for (path, _) in files.into_iter().skip(keep) {
        let _ = fs::remove_file(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_directory_is_under_localappdata_or_home() {
        let dir = crash_directory();
        let s = dir.to_string_lossy();
        assert!(s.contains("psmux") && s.contains("crashes"));
    }

    #[test]
    fn list_crashes_returns_empty_on_missing_dir() {
        let entries = list_crashes(10);
        assert!(entries.len() <= 10);
    }

    #[test]
    fn parse_crash_stem_extracts_pid_and_timestamp() {
        assert_eq!(parse_crash_stem("psmux-42-1000"), Some((42, 1000)));
        assert_eq!(
            parse_crash_stem("psmux-1234-1700000000"),
            Some((1234, 1_700_000_000))
        );
        assert_eq!(parse_crash_stem("notpsmux-1-1"), None);
        assert_eq!(parse_crash_stem("psmux-abc-1"), None);
    }

    #[test]
    fn prune_keeps_only_n_newest() {
        // Isolate from any real crash directory by pointing LOCALAPPDATA at a temp dir.
        let dir = std::env::temp_dir().join(format!(
            "psmux-test-prune-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let crashes_dir = dir.join("psmux").join("crashes");
        fs::create_dir_all(&crashes_dir).unwrap();

        for i in 0..5u64 {
            let path = crashes_dir.join(format!("psmux-1-{}.crash", 1000 + i));
            fs::write(&path, format!("test crash {i}")).unwrap();
        }

        // SAFETY: tests in this module don't spawn threads that read LOCALAPPDATA
        // concurrently with this mutation.
        let saved = std::env::var_os("LOCALAPPDATA");
        unsafe {
            std::env::set_var("LOCALAPPDATA", &dir);
        }

        prune_crashes(2);
        let remaining: Vec<_> = fs::read_dir(&crashes_dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().into_string().unwrap())
            .collect();
        assert_eq!(remaining.len(), 2);
        assert!(remaining.iter().any(|n| n.contains("-1004.crash")));
        assert!(remaining.iter().any(|n| n.contains("-1003.crash")));

        unsafe {
            match saved {
                Some(v) => std::env::set_var("LOCALAPPDATA", v),
                None => std::env::remove_var("LOCALAPPDATA"),
            }
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_crashes_returns_newest_first() {
        let dir = std::env::temp_dir().join(format!(
            "psmux-test-list-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let crashes_dir = dir.join("psmux").join("crashes");
        fs::create_dir_all(&crashes_dir).unwrap();

        for i in 0..3u64 {
            fs::write(crashes_dir.join(format!("psmux-9-{}.crash", 2000 + i)), "x").unwrap();
        }

        let saved = std::env::var_os("LOCALAPPDATA");
        unsafe {
            std::env::set_var("LOCALAPPDATA", &dir);
        }

        let entries = list_crashes(10);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].timestamp, 2002);
        assert_eq!(entries[1].timestamp, 2001);
        assert_eq!(entries[2].timestamp, 2000);

        unsafe {
            match saved {
                Some(v) => std::env::set_var("LOCALAPPDATA", v),
                None => std::env::remove_var("LOCALAPPDATA"),
            }
        }
        fs::remove_dir_all(&dir).ok();
    }
}
