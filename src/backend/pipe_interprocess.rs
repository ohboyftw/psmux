//! SPIKE: Parallel named-pipe listener built on the `interprocess` crate.
//!
//! Behind `--features interprocess-pipe` (default OFF). Sibling of
//! [`super::pipe`] — same public surface (`pipe_path`, `start_pipe_listener`),
//! same drop-guard semantics for the push-event sender (commit `aaae38f`).
//!
//! Goal of the spike: prove or disprove that we can replace the hand-rolled
//! Win32 `CreateNamedPipeW`/`ConnectNamedPipe` code in [`super::pipe`] with the
//! safe wrapper from `interprocess`. Findings live in
//! `.claude/internal/spike-interprocess-pipe.md`.
//!
//! API differences vs. `super::pipe`:
//! - `PipeListenerOptions` builder returns a `PipeListener<DuplexBytePipeStream>`
//!   from a single `.create()` call — no manual `INVALID_HANDLE_VALUE` check.
//! - `accept()` already handles the `ERROR_PIPE_CONNECTED` race internally.
//! - `DuplexBytePipeStream` has no `try_clone`, so we duplicate the underlying
//!   `HANDLE` ourselves to give the writer thread its own end (matches how
//!   `super::pipe` calls `file.try_clone()?`).

#![cfg(all(windows, feature = "interprocess-pipe"))]

use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, RawHandle};
use std::sync::mpsc;
use std::thread;

use interprocess::os::windows::named_pipe::{
    DuplexBytePipeStream, PipeListener, PipeListenerOptions, PipeMode,
};

use crate::types::CtrlReq;

/// Named pipe path for a session — identical to [`super::pipe::pipe_path`] so
/// clients can find the pipe regardless of which backend the server uses.
pub fn pipe_path(session_name: &str) -> String {
    format!(r"\\.\pipe\psmux-claude-backend-{}", session_name)
}

/// The bare pipe name (no `\\.\pipe\` prefix) — `interprocess` prepends the
/// prefix internally in `convert_path`, so we hand it the bare name.
fn bare_pipe_name(session_name: &str) -> String {
    format!("psmux-claude-backend-{}", session_name)
}

fn psmux_dir() -> String {
    crate::paths::psmux_dir()
}

/// Start a `PipeListener` and dispatch each accepted connection.
///
/// Mirrors [`super::pipe::start_pipe_listener`]. The listener keeps producing
/// `DuplexBytePipeStream`s; each is handled on its own thread.
#[allow(unused)]
pub fn start_pipe_listener(
    session_name: &str,
    tx: mpsc::Sender<CtrlReq>,
    _session_key: String,
) -> io::Result<()> {
    let bare = bare_pipe_name(session_name);
    let full = pipe_path(session_name);

    let listener: PipeListener<DuplexBytePipeStream> = PipeListenerOptions::new()
        .name(std::borrow::Cow::Owned(OsStr::new(&bare).to_os_string()))
        .mode(PipeMode::Bytes)
        .input_buffer_size_hint(65536_usize)
        .output_buffer_size_hint(65536_usize)
        .create()?;

    thread::spawn(move || {
        for conn in listener.incoming() {
            match conn {
                Ok(stream) => {
                    let tx = tx.clone();
                    thread::spawn(move || {
                        if let Err(e) = handle_rpc_connection(stream, tx) {
                            eprintln!("RPC connection error (interprocess): {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Pipe accept error (interprocess): {}", e);
                    thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
    });

    let dir = psmux_dir();
    let _ = std::fs::create_dir_all(&dir);
    let pipe_file = format!("{}\\{}.pipe", dir, session_name);
    let _ = std::fs::write(&pipe_file, &full);

    Ok(())
}

/// Duplicate a Windows `HANDLE` so reader and writer threads get independent
/// ends of the same pipe — `DuplexBytePipeStream` has no `try_clone()`.
///
/// Inlined `extern "system"` declarations follow the same pattern as
/// [`crate::debug_log`] so we don't need to add `Win32_System_Threading` to
/// the `windows-sys` feature list just for this spike.
fn duplicate_handle(src: RawHandle) -> io::Result<RawHandle> {
    type HANDLE = isize;
    const DUPLICATE_SAME_ACCESS: u32 = 0x0000_0002;
    extern "system" {
        fn DuplicateHandle(
            source_process_handle: HANDLE,
            source_handle: HANDLE,
            target_process_handle: HANDLE,
            target_handle: *mut HANDLE,
            desired_access: u32,
            inherit_handle: i32,
            options: u32,
        ) -> i32;
        fn GetCurrentProcess() -> HANDLE;
    }

    let mut new_handle: HANDLE = 0;
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close.
    // `src` is a valid pipe HANDLE that we obtained from `as_raw_handle` on a
    // live DuplexBytePipeStream. DuplicateHandle writes into `new_handle` and
    // returns nonzero on success — we check before using.
    let ok = unsafe {
        let cur = GetCurrentProcess();
        DuplicateHandle(
            cur,
            src as HANDLE,
            cur,
            &mut new_handle,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(new_handle as RawHandle)
}

/// Drive one accepted pipe connection: split the stream into a reader half and
/// a writer half, register the push-event sender behind a drop-guard (commit
/// `aaae38f`), and run the JSON-RPC request/response loop.
fn handle_rpc_connection(
    stream: DuplexBytePipeStream,
    tx: mpsc::Sender<CtrlReq>,
) -> io::Result<()> {
    use super::dispatcher::dispatch_rpc;
    use std::sync::Arc;

    // Duplicate the handle so reader and writer threads each own a File.
    let raw = stream.as_raw_handle();
    let dup = duplicate_handle(raw)?;
    // Move ownership of the original handle out of the stream — we wrap it as
    // a File for the reader. The duplicate becomes the writer.
    let original = stream.into_raw_handle();
    // SAFETY: both handles are valid Win32 named-pipe handles owned by us;
    // wrapping them in File transfers ownership and CloseHandle on drop.
    let reader_file = unsafe { File::from_raw_handle(original) };
    let writer_file = unsafe { File::from_raw_handle(dup) };

    let writer = Arc::new(std::sync::Mutex::new(writer_file));
    let writer_for_events = Arc::clone(&writer);

    // Drop-guard pattern from commit aaae38f: when this function returns,
    // `_event_reg` drops, which removes the sender from the global registry,
    // closes the channel, and lets the writer thread below exit. Without it
    // the sender would leak and the thread would zombie until
    // `push_backend_event` eventually fails to write.
    let (event_tx, event_rx) = mpsc::channel::<String>();
    let _event_reg = crate::types::register_backend_event_sender(event_tx);

    thread::spawn(move || {
        while let Ok(event_json) = event_rx.recv() {
            if let Ok(mut w) = writer_for_events.lock() {
                if writeln!(w, "{}", event_json).is_err() {
                    break;
                }
                if w.flush().is_err() {
                    break;
                }
            }
        }
    });

    let reader = BufReader::new(reader_file);
    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        if let Some(resp_json) = dispatch_rpc(&line, &tx) {
            let mut w = writer.lock().unwrap();
            writeln!(w, "{}", resp_json)?;
            w.flush()?;
        }
    }

    Ok(())
}
