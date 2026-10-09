use super::job::PaneJob;
use super::WinChild;
use crate::cmdbuilder::CommandBuilder;
use crate::win::procthreadattr::ProcThreadAttributeList;
use anyhow::{bail, ensure, Error};
use filedescriptor::{FileDescriptor, OwnedHandle};
use lazy_static::lazy_static;
use std::ffi::OsString;
use std::io::Error as IoError;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::{mem, ptr};
use winapi::shared::minwindef::{DWORD, FARPROC, HMODULE};
use winapi::shared::winerror::ERROR_ACCESS_DENIED;
use winapi::shared::winerror::{HRESULT, S_OK};
use winapi::um::handleapi::*;
use winapi::um::libloaderapi::{GetModuleHandleW, GetProcAddress, LoadLibraryExW};
use winapi::um::processthreadsapi::*;
use winapi::um::winbase::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT,
    STARTF_USESTDHANDLES, STARTUPINFOEXW,
};
use winapi::um::wincon::COORD;
use winapi::um::winnt::HANDLE;

#[allow(clippy::upper_case_acronyms)]
pub type HPCON = HANDLE;

pub const PSUEDOCONSOLE_INHERIT_CURSOR: DWORD = 0x1;
pub const PSEUDOCONSOLE_RESIZE_QUIRK: DWORD = 0x2;
pub const PSEUDOCONSOLE_WIN32_INPUT_MODE: DWORD = 0x4;
pub const PSEUDOCONSOLE_PASSTHROUGH_MODE: DWORD = 0x8;
/// OpenConsole 1.22+ only, where bits 0x18 select how the host measures text
/// width and 0x8 no longer means passthrough.  wcswidth is per-codepoint,
/// which is how the vt100-psmux parser measures.
pub const PSEUDOCONSOLE_GLYPH_WIDTH_WCSWIDTH: DWORD = 0x10;

/// Where the three ConPTY entry points came from.
///
/// Upstream psmux has only `Kernel32` and `Directory`.  Code ported from
/// upstream that asks "is the pane host user supplied" (`== Directory`)
/// means "is it OpenConsole rather than the inbox conhost" here, which is
/// `!= Kernel32`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConPtySource {
    /// The system implementation in kernel32.dll, which drives the inbox
    /// conhost.exe.  Used when no pinned host is present or one fails to load.
    Kernel32,
    /// The pinned conpty.dll in `conpty\` beside the psmux executable, which
    /// drives the OpenConsole.exe next to it.  The default when present.
    Bundled,
    /// A conpty.dll the user named with `PSMUX_CONPTY_DIR`, which drives the
    /// OpenConsole.exe next to it.  Overrides the pinned one.
    Directory,
}

type CreateFn = unsafe extern "system" fn(COORD, HANDLE, HANDLE, DWORD, *mut HPCON) -> HRESULT;
type ResizeFn = unsafe extern "system" fn(HPCON, COORD) -> HRESULT;
type CloseFn = unsafe extern "system" fn(HPCON);

/// The three ConPTY entry points plus a note of where they came from.
///
/// The module behind the pointers is never freed: this lives in a
/// `lazy_static` for the life of the process and panes hold HPCONs it made.
#[allow(non_snake_case)]
struct ConPtyApi {
    CreatePseudoConsole: CreateFn,
    ResizePseudoConsole: ResizeFn,
    ClosePseudoConsole: CloseFn,
    source: ConPtySource,
}

/// Look the three entry points up in `module`, a loaded module that is never
/// freed.  `label` names it in the error.
fn resolve_exports(
    module: HMODULE,
    label: &str,
    source: ConPtySource,
) -> Result<ConPtyApi, String> {
    let resolve = |name: &[u8]| {
        // SAFETY: `module` is loaded and `name` is NUL terminated.
        let p = unsafe { GetProcAddress(module, name.as_ptr() as *const i8) };
        if p.is_null() {
            let name = String::from_utf8_lossy(&name[..name.len() - 1]);
            return Err(format!("{} exports no {}", label, name));
        }
        Ok(p)
    };
    let create = resolve(b"CreatePseudoConsole\0")?;
    let resize = resolve(b"ResizePseudoConsole\0")?;
    let close = resolve(b"ClosePseudoConsole\0")?;
    // SAFETY: kernel32 and conpty.dll export these three with exactly these
    // signatures, and the module stays loaded for the life of the process.
    Ok(unsafe {
        ConPtyApi {
            CreatePseudoConsole: mem::transmute::<FARPROC, CreateFn>(create),
            ResizePseudoConsole: mem::transmute::<FARPROC, ResizeFn>(resize),
            ClosePseudoConsole: mem::transmute::<FARPROC, CloseFn>(close),
            source,
        }
    })
}

fn load_conpty_kernel32() -> ConPtyApi {
    let name: Vec<u16> = "kernel32.dll".encode_utf16().chain(Some(0)).collect();
    // SAFETY: `name` is NUL terminated; kernel32 is mapped into every process
    // and never unloaded.
    let module = unsafe { GetModuleHandleW(name.as_ptr()) };
    resolve_exports(module, "kernel32.dll", ConPtySource::Kernel32).expect(
        "this system does not support conpty.  Windows 10 October 2018 or newer is required",
    )
}

/// Load `<dir>\conpty.dll` by absolute path with
/// `LOAD_WITH_ALTERED_SEARCH_PATH`, so the DLL resolves against its own
/// directory and drives the OpenConsole.exe beside it.  Never by bare name:
/// terminal emulators such as WezTerm ship their own conpty.dll, the DLL
/// search order can pick it up when psmux runs inside them, and an untested
/// OpenConsole.exe causes blank panes and broken I/O.
///
/// OpenConsole.exe must be there too: without it conpty.dll silently
/// launches the inbox conhost and hands it flags it does not understand.
///
/// Returns a human readable reason on failure.
fn load_conpty_from_dir(dir: &Path, source: ConPtySource) -> Result<ConPtyApi, String> {
    const LOAD_WITH_ALTERED_SEARCH_PATH: DWORD = 0x0000_0008;

    if !dir.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    for file in ["conpty.dll", "OpenConsole.exe"] {
        if !dir.join(file).is_file() {
            return Err(format!("{} does not exist", dir.join(file).display()));
        }
    }
    // LOAD_WITH_ALTERED_SEARCH_PATH requires an absolute path to mean anything.
    let abs = std::fs::canonicalize(dir.join("conpty.dll"))
        .map_err(|e| format!("cannot resolve {}: {}", dir.display(), e))?;
    let wide: Vec<u16> = abs.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a NUL terminated absolute path.
    let module = unsafe {
        LoadLibraryExW(
            wide.as_ptr(),
            ptr::null_mut(),
            LOAD_WITH_ALTERED_SEARCH_PATH,
        )
    };
    if module.is_null() {
        let err = IoError::last_os_error();
        return Err(format!("LoadLibraryExW({}) failed: {}", abs.display(), err));
    }
    // The module is intentionally never freed: see ConPtyApi.
    resolve_exports(module, &abs.display().to_string(), source)
}

/// Pick the ConPTY implementation, first match wins:
///
/// 1. `env_dir`, the value of `PSMUX_CONPTY_DIR`.  An explicit choice, so
///    when it cannot be loaded the reason is logged and psmux uses kernel32,
///    as upstream does, rather than another OpenConsole.
/// 2. `bundled_dir`, `conpty\` beside the executable.  The inbox conhost
///    (10.0.26100.9549) access-violates in COOKED_READ_DATA when a TUI that
///    used the alternate screen exits (OpenCode 2.x), killing the pane;
///    OpenConsole 1.23.2510.08001 does not.  No such directory is the normal
///    case for an install without one and is not logged as a problem.
/// 3. kernel32.
fn resolve_conpty(env_dir: Option<PathBuf>, bundled_dir: Option<PathBuf>) -> ConPtyApi {
    let (dir, source) = match (env_dir, bundled_dir) {
        (Some(dir), _) => (dir, ConPtySource::Directory),
        (None, Some(dir)) if dir.exists() => (dir, ConPtySource::Bundled),
        _ => return load_conpty_kernel32(),
    };
    match load_conpty_from_dir(&dir, source) {
        Ok(api) => {
            log::info!("ConPTY loaded from {} ({:?})", dir.display(), source);
            api
        }
        Err(reason) => {
            log::warn!(
                "ConPTY directory {} not used ({}); falling back to kernel32",
                dir.display(),
                reason
            );
            load_conpty_kernel32()
        }
    }
}

/// `PSMUX_CONPTY_DIR` as a directory; unset and empty both mean "not named".
fn conpty_dir_from_env(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn bundled_conpty_dir() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("conpty"))
}

lazy_static! {
    static ref CONPTY: ConPtyApi = resolve_conpty(
        conpty_dir_from_env(std::env::var_os("PSMUX_CONPTY_DIR")),
        bundled_conpty_dir(),
    );
}

/// Which ConPTY implementation this process loaded.  Diagnostics only.
pub fn conpty_source() -> ConPtySource {
    CONPTY.source
}

pub struct PsuedoCon {
    con: HPCON,
    /// Whether this ConPTY was created with PSEUDOCONSOLE_PASSTHROUGH_MODE.
    /// Used by the retry logic in ConPtySlavePty::spawn_command to decide
    /// whether a fallback without passthrough is worth attempting.
    pub used_passthrough: bool,
}

unsafe impl Send for PsuedoCon {}
unsafe impl Sync for PsuedoCon {}

impl Drop for PsuedoCon {
    fn drop(&mut self) {
        unsafe { (CONPTY.ClosePseudoConsole)(self.con) };
    }
}

/// Returns true if the current Windows build supports ConPTY passthrough mode.
/// PSEUDOCONSOLE_PASSTHROUGH_MODE requires Windows 11 22H2 (build 22621+).
/// On older Windows versions, the flag may be silently accepted but produce
/// broken ConPTY output (no Win32 Console API translation).
///
/// Respects `PSMUX_NO_PASSTHROUGH=1` environment variable to let users
/// force-disable passthrough mode on builds where it causes CreateProcessW
/// to fail with ERROR_INVALID_PARAMETER (87).
fn supports_passthrough_mode() -> bool {
    if std::env::var("PSMUX_NO_PASSTHROUGH")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        log::info!("ConPTY passthrough mode disabled via PSMUX_NO_PASSTHROUGH");
        return false;
    }
    // An OpenConsole conpty.dll (pinned or PSMUX_CONPTY_DIR, assumed 1.22+)
    // has no passthrough flag: 0x8 is a text-width mode
    // there, and its host relays VT sequences unconditionally.
    if CONPTY.source != ConPtySource::Kernel32 {
        return false;
    }
    let ver = unsafe {
        let mut info: winapi::um::winnt::OSVERSIONINFOW = mem::zeroed();
        info.dwOSVersionInfoSize = mem::size_of::<winapi::um::winnt::OSVERSIONINFOW>() as u32;
        // RtlGetVersion is used because GetVersionEx lies on Windows 10+
        // unless the application has a compatibility manifest.
        type RtlGetVersionFn =
            unsafe extern "system" fn(*mut winapi::um::winnt::OSVERSIONINFOW) -> i32;
        let ntdll = winapi::um::libloaderapi::GetModuleHandleW(
            [
                'n' as u16, 't' as u16, 'd' as u16, 'l' as u16, 'l' as u16, '.' as u16, 'd' as u16,
                'l' as u16, 'l' as u16, 0,
            ]
            .as_ptr(),
        );
        if ntdll.is_null() {
            return false;
        }
        let func = winapi::um::libloaderapi::GetProcAddress(
            ntdll,
            b"RtlGetVersion\0".as_ptr() as *const i8,
        );
        if func.is_null() {
            return false;
        }
        let rtl_get_version: RtlGetVersionFn = mem::transmute(func);
        rtl_get_version(&mut info);
        info
    };
    // Windows 11 22H2 = build 22621
    ver.dwBuildNumber >= 22621
}

/// Flags every pseudoconsole is created with.
fn base_flags() -> DWORD {
    let flags =
        PSUEDOCONSOLE_INHERIT_CURSOR | PSEUDOCONSOLE_RESIZE_QUIRK | PSEUDOCONSOLE_WIN32_INPUT_MODE;
    if CONPTY.source != ConPtySource::Kernel32 {
        flags | PSEUDOCONSOLE_GLYPH_WIDTH_WCSWIDTH
    } else {
        flags
    }
}

impl PsuedoCon {
    pub fn new(size: COORD, input: FileDescriptor, output: FileDescriptor) -> Result<Self, Error> {
        let mut con: HPCON = INVALID_HANDLE_VALUE;
        let base_flags = base_flags();

        // Use PSEUDOCONSOLE_PASSTHROUGH_MODE on Windows 11 22H2+ to relay
        // VT sequences (including DECSCUSR cursor shapes) from child processes
        // directly through the output pipe.  On older Windows, this flag is
        // silently accepted but breaks Win32 Console API translation, so we
        // only attempt it on known-good builds.
        if supports_passthrough_mode() {
            let result = unsafe {
                (CONPTY.CreatePseudoConsole)(
                    size,
                    input.as_raw_handle() as _,
                    output.as_raw_handle() as _,
                    base_flags | PSEUDOCONSOLE_PASSTHROUGH_MODE,
                    &mut con,
                )
            };

            if result == S_OK {
                return Ok(Self {
                    con,
                    used_passthrough: true,
                });
            }
            // If the API call failed despite being on a supported build,
            // fall through to the standard path.
            con = INVALID_HANDLE_VALUE;
        }

        let result = unsafe {
            (CONPTY.CreatePseudoConsole)(
                size,
                input.as_raw_handle() as _,
                output.as_raw_handle() as _,
                base_flags,
                &mut con,
            )
        };
        ensure!(
            result == S_OK,
            "failed to create psuedo console: HRESULT {}",
            result
        );
        Ok(Self {
            con,
            used_passthrough: false,
        })
    }

    /// Create a ConPTY explicitly without passthrough mode, regardless of
    /// Windows build version.  Used by the retry logic when CreateProcessW
    /// rejects the passthrough ConPTY handle.
    pub fn new_without_passthrough(
        size: COORD,
        input: FileDescriptor,
        output: FileDescriptor,
    ) -> Result<Self, Error> {
        let mut con: HPCON = INVALID_HANDLE_VALUE;
        let base_flags = base_flags();

        let result = unsafe {
            (CONPTY.CreatePseudoConsole)(
                size,
                input.as_raw_handle() as _,
                output.as_raw_handle() as _,
                base_flags,
                &mut con,
            )
        };
        ensure!(
            result == S_OK,
            "failed to create psuedo console (no passthrough): HRESULT {}",
            result
        );
        Ok(Self {
            con,
            used_passthrough: false,
        })
    }

    pub fn resize(&self, size: COORD) -> Result<(), Error> {
        let result = unsafe { (CONPTY.ResizePseudoConsole)(self.con, size) };
        ensure!(
            result == S_OK,
            "failed to resize console to {}x{}: HRESULT: {}",
            size.X,
            size.Y,
            result
        );
        Ok(())
    }

    pub fn spawn_command(&self, cmd: CommandBuilder) -> anyhow::Result<WinChild> {
        let (exe, cmdline) = cmd.cmdline()?;
        let mut launch = Launch {
            exe,
            cmdline,
            env: cmd.environment_block(),
            cwd: cmd.current_directory(),
        };
        let (pi, job) = match launch.create_in_job(self.con) {
            Ok(created) => created,
            Err(err) => {
                let msg = format!(
                    "CreateProcessW `{:?}` in cwd `{:?}` failed: {}",
                    OsString::from_wide(&launch.cmdline),
                    launch.cwd.as_ref().map(|c| OsString::from_wide(c)),
                    err
                );
                log::error!("{}", msg);
                bail!("{}", msg);
            }
        };

        // Make sure we close out the thread handle so we don't leak it;
        // we do this simply by making it owned
        // SAFETY: CreateProcessW succeeded, so both handles are open and
        // owned by us alone.
        let _main_thread = unsafe { OwnedHandle::from_raw_handle(pi.hThread as _) };
        // SAFETY: as above.
        let proc = unsafe { OwnedHandle::from_raw_handle(pi.hProcess as _) };

        Ok(WinChild {
            proc: Mutex::new(proc),
            _job: job,
        })
    }
}

/// Images for which CreateProcessW refused a job list (see create_in_job).
static JOBLIST_REFUSED: Mutex<Vec<Vec<u16>>> = Mutex::new(Vec::new());

/// Whether `exe` is known to refuse the job list attribute.  An app execution
/// alias (a WindowsApps path, as the Store pwsh is launched) always does, so
/// it skips the attempt that would otherwise cost each server's first spawn.
fn joblist_refused(exe: &[u16]) -> bool {
    let lower = String::from_utf16_lossy(exe).to_ascii_lowercase();
    if lower.contains("\\windowsapps\\") {
        return true;
    }
    JOBLIST_REFUSED
        .lock()
        .map(|v| v.iter().any(|e| e.as_slice() == exe))
        .unwrap_or(false)
}

fn remember_joblist_refused(exe: &[u16]) {
    if let Ok(mut v) = JOBLIST_REFUSED.lock() {
        if !v.iter().any(|e| e.as_slice() == exe) {
            v.push(exe.to_vec());
        }
    }
}

/// The CreateProcessW inputs for one pane process.
struct Launch {
    exe: Vec<u16>,
    cmdline: Vec<u16>,
    env: Vec<u16>,
    cwd: Option<Vec<u16>>,
}

impl Launch {
    /// Create the process on `con` inside a fresh [`PaneJob`], best way first:
    ///
    /// 1. The job list attribute: the process is born in its job, so nothing
    ///    it starts can run outside it.
    /// 2. Windows refuses a job list for some images with ERROR_ACCESS_DENIED
    ///    (measured upstream for the WindowsApps alias of the Store pwsh when
    ///    the caller is a detached server).  Those are created suspended,
    ///    assigned to the job, then resumed.  The refused attempt leaves its
    ///    job unusable for the assign, so this path gets a fresh one.
    /// 3. Without a job, exactly as before the job existed.
    fn create_in_job(
        &mut self,
        con: HPCON,
    ) -> Result<(PROCESS_INFORMATION, Option<PaneJob>), IoError> {
        let mut job = PaneJob::create();
        if let Some(j) = job.as_ref().filter(|_| !joblist_refused(&self.exe)) {
            match self.create(con, Some(j), 0) {
                Ok(pi) => return Ok((pi, job)),
                Err(e) => {
                    if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
                        remember_joblist_refused(&self.exe);
                    }
                    log::info!("CreateProcessW with a job list failed ({}); retrying", e);
                    job = PaneJob::create();
                }
            }
        }
        if let Some(j) = job {
            if let Ok(pi) = self.create(con, None, CREATE_SUSPENDED) {
                let job = j.assign(pi.hProcess).then_some(j);
                // SAFETY: pi.hThread is the open, suspended main thread of
                // the process just created.
                unsafe { ResumeThread(pi.hThread) };
                return Ok((pi, job));
            }
        }
        self.create(con, None, 0).map(|pi| (pi, None))
    }

    fn create(
        &mut self,
        con: HPCON,
        job: Option<&PaneJob>,
        extra: DWORD,
    ) -> Result<PROCESS_INFORMATION, IoError> {
        let mut attrs = ProcThreadAttributeList::with_capacity(if job.is_some() { 2 } else { 1 })
            .map_err(IoError::other)?;
        attrs.set_pty(con).map_err(IoError::other)?;
        if let Some(j) = job {
            attrs.set_job(j.handle()).map_err(IoError::other)?;
        }
        let mut si = startup_info();
        si.lpAttributeList = attrs.as_mut_ptr();
        // SAFETY: PROCESS_INFORMATION is plain data that CreateProcessW fills.
        let mut pi: PROCESS_INFORMATION = unsafe { mem::zeroed() };
        // SAFETY: every buffer is NUL terminated and owned by `self`, the
        // attribute list outlives the call, and bInheritHandles is FALSE:
        // the pseudoconsole attribute connects the child's stdio by itself.
        let res = unsafe {
            CreateProcessW(
                self.exe.as_mut_ptr(),
                self.cmdline.as_mut_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | extra,
                self.env.as_mut_ptr() as *mut _,
                self.cwd.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
                &mut si.StartupInfo,
                &mut pi,
            )
        };
        if res == 0 {
            return Err(IoError::last_os_error());
        }
        Ok(pi)
    }
}

/// A STARTUPINFOEXW whose std handles are explicitly invalid.  Otherwise the
/// child can inherit the parent's redirected stdio: when the server's stdio
/// goes to a log file, the pane would write there instead of to the pty.
fn startup_info() -> STARTUPINFOEXW {
    // SAFETY: STARTUPINFOEXW is plain data; all-zero is its documented
    // starting state.
    let mut si: STARTUPINFOEXW = unsafe { mem::zeroed() };
    si.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as u32;
    si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    si.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    si.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    si.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    si
}

#[cfg(test)]
#[path = "psuedocon_tests.rs"]
mod tests;
