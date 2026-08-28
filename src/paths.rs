//! Single source of truth for psmux's home and data directories.
//!
//! Every `.port` / `.key` / `.version` / `.pipe` file, every log, and the
//! plugin, resurrect and crash directories resolve through [`psmux_dir`] (or
//! [`psmux_dir_opt`]), so client and server can never disagree about where the
//! bookkeeping lives — and so `PSMUX_DATA_DIR` moves all of it at once.
//!
//! Config files (`~/.psmux.conf`, `~/.psmuxrc`) are deliberately NOT in here:
//! they live next to the user's other dotfiles, not in the data directory, and
//! an embedder redirecting its server state must not silently stop reading the
//! user's config. Those sites use [`home_dir`].

/// User home directory: `USERPROFILE`, then the Windows profile API, then
/// `HOMEDRIVE`+`HOMEPATH`, then `HOME`. Empty string when nothing resolves.
///
/// `HOME` is deliberately the LAST resort on Windows: MSYS2 login shells unset
/// `USERPROFILE` and set `HOME` to a POSIX-style path (`/home/user`), so a
/// psmux invocation from an MSYS2 shell that trusted `HOME` would resolve the
/// data dir somewhere the port files do not live, and see every live server as
/// untracked. Querying the profile API keeps every invocation of the same user
/// converging on the same data dir regardless of which shell launched it.
pub fn home_dir() -> String {
    if let Ok(v) = std::env::var("USERPROFILE") {
        if !v.is_empty() {
            return v;
        }
    }
    #[cfg(windows)]
    if let Some(p) = windows_profile_dir() {
        return p;
    }
    let drive = std::env::var("HOMEDRIVE").unwrap_or_default();
    let path = std::env::var("HOMEPATH").unwrap_or_default();
    if !drive.is_empty() && !path.is_empty() {
        let p = format!("{}{}", drive, path);
        if std::path::Path::new(&p).is_dir() {
            return p;
        }
    }
    std::env::var("HOME").unwrap_or_default()
}

/// The current user's profile directory straight from the OS
/// (`GetUserProfileDirectoryW` on the process token), independent of any
/// environment variable a shell may have rewritten or unset.
#[cfg(windows)]
fn windows_profile_dir() -> Option<String> {
    use std::ffi::c_void;
    // isize handles throughout, to match every other declaration of these in
    // the crate (clashing_extern_declarations is deny-by-default here).
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn OpenProcessToken(process: isize, access: u32, token: *mut *mut c_void) -> i32;
        fn CloseHandle(h: isize) -> i32;
    }
    #[link(name = "userenv")]
    extern "system" {
        fn GetUserProfileDirectoryW(token: *mut c_void, buf: *mut u16, len: *mut u32) -> i32;
    }
    const TOKEN_QUERY: u32 = 0x0008;
    // SAFETY: the token is opened on this process, closed on every path out,
    // and the buffer length handed to GetUserProfileDirectoryW is the real
    // capacity of `buf`. The call only writes within that capacity and reports
    // the written length in `len`.
    unsafe {
        let mut token: *mut c_void = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = GetUserProfileDirectoryW(token, buf.as_mut_ptr(), &mut len);
        CloseHandle(token as isize);
        if ok == 0 || len == 0 {
            return None;
        }
        let s = String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)]);
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
}

/// psmux data directory, without a trailing separator. Fallible variant, for
/// call sites that early-exit when there is no home directory.
///
/// An absolute `PSMUX_DATA_DIR` takes precedence over the home directory, so an
/// embedding application can keep server state, logs and session files inside
/// its own install root. A relative value is rejected rather than resolved
/// against the current directory: the client and the server it talks to run
/// from different working directories, and a data dir that depended on `cwd`
/// would silently split them into two universes.
pub fn psmux_dir_opt() -> Option<String> {
    if let Some(raw) = std::env::var_os("PSMUX_DATA_DIR") {
        let path = std::path::PathBuf::from(raw);
        assert!(
            path.is_absolute() && !path.as_os_str().is_empty(),
            "PSMUX_DATA_DIR must be an absolute non-empty path"
        );
        return Some(
            path.to_string_lossy()
                .trim_end_matches(['/', '\\'])
                .to_string(),
        );
    }
    let home = home_dir();
    if home.is_empty() {
        None
    } else {
        Some(format!("{}\\.psmux", home))
    }
}

/// psmux data directory, without a trailing separator. Infallible variant, for
/// call sites that historically used `.unwrap_or_default()` on the home lookup.
///
/// With no home directory this returns `\.psmux` — byte-identical to the
/// `format!("{}\\.psmux", "")` those sites used to build. It does not panic:
/// the historical behaviour is to proceed and let the filesystem call fail,
/// and crashing on an unset `USERPROFILE` would be worse.
pub fn psmux_dir() -> String {
    psmux_dir_opt().unwrap_or_else(|| "\\.psmux".to_string())
}

/// Path to a fixed-name entry directly under the data directory — a log
/// (`latency.log`), a marker (`last_session`), or a subdirectory
/// (`plugins`, `resurrect`, `crashes`).
pub fn psmux_dir_file(name: impl AsRef<str>) -> String {
    format!("{}\\{}", psmux_dir(), name.as_ref())
}

/// Path to a session's `.port` file (the TCP port its server listens on).
pub fn port_file(session: impl AsRef<str>) -> String {
    format!("{}\\{}.port", psmux_dir(), session.as_ref())
}

/// Path to a session's `.key` file (the auth key for its server).
pub fn key_file(session: impl AsRef<str>) -> String {
    format!("{}\\{}.key", psmux_dir(), session.as_ref())
}

/// Path to a session's `.version` file (the binary version that started it).
pub fn version_file(session: impl AsRef<str>) -> String {
    format!("{}\\{}.version", psmux_dir(), session.as_ref())
}

/// Path to a session's `.pipe` discovery file (the named pipe the pane backend
/// listens on; the Pi adapter reads this to find a running session).
pub fn pipe_file(session: impl AsRef<str>) -> String {
    format!("{}\\{}.pipe", psmux_dir(), session.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_dir_is_dot_psmux_under_home() {
        let dir = psmux_dir();
        assert!(dir.ends_with("\\.psmux"), "got {dir:?}");
        assert_eq!(psmux_dir_opt().as_deref(), Some(dir.as_str()));
    }

    #[test]
    fn per_session_helpers_append_the_name_and_suffix() {
        let dir = psmux_dir();
        assert_eq!(port_file("foo"), format!("{}\\foo.port", dir));
        assert_eq!(key_file("foo"), format!("{}\\foo.key", dir));
        assert_eq!(version_file("foo"), format!("{}\\foo.version", dir));
        assert_eq!(pipe_file("foo"), format!("{}\\foo.pipe", dir));
    }

    #[test]
    fn a_fixed_name_hangs_directly_off_the_data_dir() {
        assert_eq!(
            psmux_dir_file("latency.log"),
            format!("{}\\latency.log", psmux_dir())
        );
    }
}
