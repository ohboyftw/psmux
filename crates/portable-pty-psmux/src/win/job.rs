use filedescriptor::OwnedHandle;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use winapi::shared::minwindef::{DWORD, LPVOID};
use winapi::um::jobapi2::{AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject};
use winapi::um::winnt::{
    JobObjectExtendedLimitInformation, HANDLE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

/// A job object holding one pane's process and everything it starts, armed
/// with KILL_ON_JOB_CLOSE while psmux holds it.
///
/// tmux hangs up a pane's processes when the pane or the server goes away.
/// On Windows the pseudoconsole's conhost plays that part when its owner
/// closes it, but not when the owner dies while that conhost is still
/// starting, and not for processes the pane started that ignore console
/// errors.  The job handle lives in the `WinChild` the server keeps per pane,
/// so when the server ends in any way the kernel closes the handle and ends
/// every process still in the job.
///
/// Each pane gets its own job, entered at creation, rather than one job for
/// the whole server: the server's own children are not reliably kept in an
/// inherited job.  The conhost is not put in a job, so it still runs its
/// CTRL_CLOSE round when it exits on its own.
///
/// BREAKAWAY_OK lets a process the pane starts leave on purpose with
/// CREATE_BREAKAWAY_FROM_JOB.  A psmux server started from inside a pane does
/// that, so it outlives the pane, as `tmux new -d` inside tmux does.
///
/// `PSMUX_NO_PANE_JOB=1` skips the job, for diagnosis.
///
/// Ported by intent from upstream psmux 41a02fc.
#[derive(Debug)]
pub(crate) struct PaneJob(OwnedHandle);

impl PaneJob {
    /// A new, empty, armed job, or `None` when the job could not be made or
    /// `PSMUX_NO_PANE_JOB=1`.  Without a job the pane runs as it did before.
    pub(crate) fn create() -> Option<PaneJob> {
        if std::env::var_os("PSMUX_NO_PANE_JOB").is_some_and(|v| v == "1") {
            return None;
        }
        // SAFETY: NULL security attributes (the handle is not inheritable, so
        // no child can keep the job alive after the server) and no name.
        let job = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
        if job.is_null() {
            log::warn!(
                "CreateJobObjectW failed: {}",
                std::io::Error::last_os_error()
            );
            return None;
        }
        // SAFETY: `job` is a fresh handle that nothing else owns.
        let owned = unsafe { OwnedHandle::from_raw_handle(job as _) };
        let flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
        if !set_limit_flags(job, flags) {
            log::warn!(
                "SetInformationJobObject failed: {}",
                std::io::Error::last_os_error()
            );
            return None;
        }
        Some(PaneJob(owned))
    }

    /// The raw job handle, for the job list attribute.  Owned by `self`.
    pub(crate) fn handle(&self) -> HANDLE {
        self.0.as_raw_handle() as _
    }

    /// Put a process that was created suspended into the job.
    pub(crate) fn assign(&self, process: HANDLE) -> bool {
        // SAFETY: both handles are open; the job is owned by `self` and the
        // caller owns `process`.
        unsafe { AssignProcessToJobObject(self.handle(), process) != 0 }
    }
}

fn set_limit_flags(job: HANDLE, flags: DWORD) -> bool {
    // SAFETY: JOBOBJECT_EXTENDED_LIMIT_INFORMATION is plain data; all-zero
    // is a valid value meaning "no limits".
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    info.BasicLimitInformation.LimitFlags = flags;
    // SAFETY: `job` is open, and `info` is a correctly sized struct of the
    // class named, alive for the duration of the call.
    unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &mut info as *mut _ as LPVOID,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as DWORD,
        ) != 0
    }
}
