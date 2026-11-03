//! Start a detached background process that inherits no handles.
//!
//! `std::process::Command` calls `CreateProcessW` with
//! `bInheritHandles = TRUE`, so every inheritable handle of the parent
//! leaks into the child for as long as it lives. Long-lived helpers (the
//! session server, the mycel bus) are started here instead.

use std::path::Path;

/// How the child's console is set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Console {
    /// A new console whose window is hidden. ConPTY needs a real console,
    /// so session servers use this.
    NewHidden,
    /// No console at all (`CREATE_NO_WINDOW`), for processes that never
    /// host a pane.
    None,
}

/// Start `exe args...` detached: a new process group, no inherited
/// handles, working directory `cwd` (the caller's when `None`). On Windows
/// it first tries to leave the caller's job, so the child outlives a pane's
/// kill-on-close job; a job that forbids breakaway refuses that, and the
/// child then starts inside it.
#[cfg(windows)]
pub fn spawn_detached(
    exe: &Path,
    args: &[String],
    console: Console,
    cwd: Option<&Path>,
) -> std::io::Result<()> {
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let base = CREATE_NEW_PROCESS_GROUP
        | match console {
            Console::NewHidden => CREATE_NEW_CONSOLE,
            Console::None => CREATE_NO_WINDOW,
        };
    let cmdline = command_line(exe, args);
    let cwd_wide: Option<Vec<u16>> = cwd.map(|d| wide(&d.as_os_str().to_string_lossy()));
    let mut last = Ok(());
    for flags in [base | CREATE_BREAKAWAY_FROM_JOB, base] {
        last = win::create_process(&cmdline, flags, console, cwd_wide.as_deref());
        if last.is_ok() {
            break;
        }
    }
    last
}

/// Start `exe args...` with no stdio, working directory `cwd` (the
/// caller's when `None`).
#[cfg(not(windows))]
pub fn spawn_detached(
    exe: &Path,
    args: &[String],
    _console: Console,
    cwd: Option<&Path>,
) -> std::io::Result<()> {
    use std::process::Stdio;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.spawn().map(drop)
}

/// `"exe" arg1 arg2 ...`, each argument quoted by [`quote_windows_arg`].
pub fn command_line(exe: &Path, args: &[String]) -> String {
    let mut cmdline = format!("\"{}\"", exe.display());
    for arg in args {
        cmdline.push(' ');
        cmdline.push_str(&quote_windows_arg(arg));
    }
    cmdline
}

/// Quote one argument for a Windows command line so `CommandLineToArgvW` and
/// the CRT read it back unchanged: backslashes are literal except before a
/// `"`, so a run of them is doubled before an escaped `"` and before the
/// closing quote.  An argument with no space, tab or `"` is passed as is.
pub fn quote_windows_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
mod win {
    use super::{wide, Console};

    #[repr(C)]
    #[allow(non_snake_case, clippy::upper_case_acronyms)]
    struct STARTUPINFOW {
        cb: u32,
        lpReserved: *mut u16,
        lpDesktop: *mut u16,
        lpTitle: *mut u16,
        dwX: u32,
        dwY: u32,
        dwXSize: u32,
        dwYSize: u32,
        dwXCountChars: u32,
        dwYCountChars: u32,
        dwFillAttribute: u32,
        dwFlags: u32,
        wShowWindow: u16,
        cbReserved2: u16,
        lpReserved2: *mut u8,
        hStdInput: isize,
        hStdOutput: isize,
        hStdError: isize,
    }

    #[repr(C)]
    #[allow(non_snake_case, clippy::upper_case_acronyms)]
    struct PROCESS_INFORMATION {
        hProcess: isize,
        hThread: isize,
        dwProcessId: u32,
        dwThreadId: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateProcessW(
            lpApplicationName: *const u16,
            lpCommandLine: *mut u16,
            lpProcessAttributes: *const std::ffi::c_void,
            lpThreadAttributes: *const std::ffi::c_void,
            bInheritHandles: i32,
            dwCreationFlags: u32,
            lpEnvironment: *const std::ffi::c_void,
            lpCurrentDirectory: *const u16,
            lpStartupInfo: *const STARTUPINFOW,
            lpProcessInformation: *mut PROCESS_INFORMATION,
        ) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }

    const STARTF_USESHOWWINDOW: u32 = 0x0000_0001;
    const SW_HIDE: u16 = 0;

    fn startup_info(console: Console) -> STARTUPINFOW {
        // SAFETY: an all-zero STARTUPINFOW is valid (null pointers, zero
        // handles); cb is set right after.
        let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        if console == Console::NewHidden {
            si.dwFlags = STARTF_USESHOWWINDOW;
            si.wShowWindow = SW_HIDE;
        }
        si
    }

    /// One `CreateProcessW` call with `bInheritHandles = FALSE`.
    pub(super) fn create_process(
        cmdline: &str,
        flags: u32,
        console: Console,
        cwd: Option<&[u16]>,
    ) -> std::io::Result<()> {
        // A fresh buffer per call: CreateProcessW may write into it.
        let mut cmdline_wide = wide(cmdline);
        let si = startup_info(console);
        // SAFETY: an all-zero PROCESS_INFORMATION is valid (zero handles).
        let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let cwd_ptr = cwd.map_or(std::ptr::null(), <[u16]>::as_ptr);
        // SAFETY: the command line and cwd are NUL terminated buffers that
        // outlive the call, `si` and `pi` are valid, and no handles are
        // inherited.
        let ok = unsafe {
            CreateProcessW(
                std::ptr::null(),
                cmdline_wide.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                flags,
                std::ptr::null(),
                cwd_ptr,
                &si,
                &mut pi,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: both handles were just returned by CreateProcessW and are
        // closed exactly once; we never wait on the child.
        unsafe {
            CloseHandle(pi.hProcess);
            CloseHandle(pi.hThread);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests-rs/test_windows_argv_quote.rs"]
mod windows_argv_quote_tests;
