use super::job::PaneJob;
use super::WinChild;
use crate::cmdbuilder::CommandBuilder;
use crate::win::procthreadattr::ProcThreadAttributeList;
use anyhow::{bail, ensure, Error};
use filedescriptor::{FileDescriptor, OwnedHandle};
use lazy_static::lazy_static;
use shared_library::shared_library;
use std::ffi::OsString;
use std::io::Error as IoError;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::{mem, ptr};
use winapi::shared::minwindef::DWORD;
use winapi::shared::winerror::ERROR_ACCESS_DENIED;
use winapi::shared::winerror::{HRESULT, S_OK};
use winapi::um::handleapi::*;
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

shared_library!(ConPtyFuncs,
    pub fn CreatePseudoConsole(
        size: COORD,
        hInput: HANDLE,
        hOutput: HANDLE,
        flags: DWORD,
        hpc: *mut HPCON
    ) -> HRESULT,
    pub fn ResizePseudoConsole(hpc: HPCON, size: COORD) -> HRESULT,
    pub fn ClosePseudoConsole(hpc: HPCON),
);

/// Directory holding a pinned conpty.dll + OpenConsole.exe pair:
/// `PSMUX_CONPTY_DIR` if set, else `conpty\` next to the psmux executable.
fn bundled_conpty_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("PSMUX_CONPTY_DIR") {
        return Some(PathBuf::from(dir));
    }
    Some(std::env::current_exe().ok()?.parent()?.join("conpty"))
}

struct ConPty {
    funcs: ConPtyFuncs,
    /// True when `funcs` comes from the pinned conpty.dll, whose flag bits
    /// differ from kernel32's.
    bundled: bool,
}

fn load_conpty() -> ConPty {
    // Prefer a pinned conpty.dll loaded by FULL PATH.  The inbox conhost.exe
    // (10.0.26100.9549) access-violates in COOKED_READ_DATA when a TUI that
    // used the alternate screen exits (OpenCode 2.x), which kills the pane.
    // OpenConsole 1.23.2510.08001 does not.
    //
    // Never load "conpty.dll" by bare name: terminal emulators like WezTerm
    // bundle their own copy, the DLL search order can pick it up when psmux
    // runs inside them, and an untested OpenConsole.exe causes blank panes
    // and broken I/O.
    //
    // OpenConsole.exe must be there too: without it conpty.dll silently
    // launches the inbox conhost and hands it flags it does not understand.
    if let Some(dir) = bundled_conpty_dir() {
        let dll = dir.join("conpty.dll");
        if dll.is_file() && dir.join("OpenConsole.exe").is_file() {
            match ConPtyFuncs::open(&dll) {
                Ok(funcs) => {
                    return ConPty {
                        funcs,
                        bundled: true,
                    }
                }
                Err(e) => log::warn!("failed to load {}: {:?}; using kernel32", dll.display(), e),
            }
        }
    }
    let funcs = ConPtyFuncs::open(Path::new("kernel32.dll")).expect(
        "this system does not support conpty.  Windows 10 October 2018 or newer is required",
    );
    ConPty {
        funcs,
        bundled: false,
    }
}

lazy_static! {
    static ref CONPTY: ConPty = load_conpty();
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
        unsafe { (CONPTY.funcs.ClosePseudoConsole)(self.con) };
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
    // The pinned conpty.dll has no passthrough flag: 0x8 is a text-width mode
    // there, and its host relays VT sequences unconditionally.
    if CONPTY.bundled {
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
    if CONPTY.bundled {
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
                (CONPTY.funcs.CreatePseudoConsole)(
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
            (CONPTY.funcs.CreatePseudoConsole)(
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
            (CONPTY.funcs.CreatePseudoConsole)(
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
        let result = unsafe { (CONPTY.funcs.ResizePseudoConsole)(self.con, size) };
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
