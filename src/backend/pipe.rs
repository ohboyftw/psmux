//! Named pipe listener for CustomPaneBackend JSON-RPC connections.
//!
//! Creates a Windows named pipe at `\\.\pipe\psmux-claude-backend-{session}`
//! and spawns a thread to accept connections. Each connection is handled in
//! its own thread, reading newline-delimited JSON-RPC requests and writing
//! responses. The dispatcher (Task 7) will route requests to session commands.

use std::io::{self, BufRead, BufReader, Write};
use std::sync::mpsc;
use std::thread;

use crate::types::CtrlReq;

/// Named pipe path for a session's CustomPaneBackend endpoint.
pub fn pipe_path(session_name: &str) -> String {
    format!(r"\\.\pipe\psmux-claude-backend-{}", session_name)
}

/// Resolve the psmux data directory (`~/.psmux/`).
fn psmux_dir() -> String {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    format!("{}\\.psmux", home)
}

/// Start listening on a named pipe for JSON-RPC connections.
///
/// Spawns a background thread that loops forever, creating pipe instances
/// and waiting for clients. Each connected client is handled in its own
/// thread. The `tx` sender allows dispatching parsed requests into the
/// session's control channel.
///
/// A discovery file is written to `~/.psmux/{session}.pipe` so that
/// clients can find the pipe path without guessing.
#[allow(unused)]
pub fn start_pipe_listener(
    session_name: &str,
    tx: mpsc::Sender<CtrlReq>,
    _session_key: String,
) -> io::Result<()> {
    let pipe_name = pipe_path(session_name);
    let pipe_name_clone = pipe_name.clone();

    thread::spawn(move || {
        loop {
            match create_and_wait_for_client(&pipe_name_clone) {
                Ok((reader, writer)) => {
                    let tx = tx.clone();
                    thread::spawn(move || {
                        if let Err(e) = handle_rpc_connection(reader, writer, tx) {
                            eprintln!("RPC connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Pipe accept error: {}", e);
                    thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
    });

    // Write pipe path to discovery file
    let dir = psmux_dir();
    let _ = std::fs::create_dir_all(&dir);
    let pipe_file = format!("{}\\{}.pipe", dir, session_name);
    let _ = std::fs::write(&pipe_file, &pipe_name);

    Ok(())
}

/// Create a named pipe instance and block until a client connects.
///
/// Returns a buffered reader and writer pair wrapping the connected pipe handle.
#[cfg(windows)]
fn create_and_wait_for_client(
    pipe_name: &str,
) -> io::Result<(BufReader<std::fs::File>, std::fs::File)> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Storage::FileSystem::*;
    use windows_sys::Win32::System::Pipes::*;

    let wide_name: Vec<u16> = pipe_name.encode_utf16().chain(std::iter::once(0)).collect();

    // SAFETY: CreateNamedPipeW is a well-documented Win32 API. We pass a valid
    // null-terminated wide string, standard pipe flags, and check the return
    // value for INVALID_HANDLE_VALUE before using the handle.
    let handle = unsafe {
        CreateNamedPipeW(
            wide_name.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            PIPE_UNLIMITED_INSTANCES,
            65536, // output buffer size
            65536, // input buffer size
            0,     // default timeout
            std::ptr::null(),
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: ConnectNamedPipe blocks until a client connects to the pipe.
    // The handle is valid (verified above). We pass null for the overlapped
    // parameter to request synchronous (blocking) operation.
    let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
    if connected == 0 {
        let err = io::Error::last_os_error();
        // ERROR_PIPE_CONNECTED means the client connected between
        // CreateNamedPipeW and ConnectNamedPipe — still a valid connection.
        if err.raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
            return Err(err);
        }
    }

    // SAFETY: handle is a valid Win32 HANDLE obtained from CreateNamedPipeW
    // and a client has connected. We transfer ownership to std::fs::File,
    // which will close the handle when dropped.
    let file = unsafe { std::fs::File::from_raw_handle(handle as *mut _) };
    let reader = BufReader::new(file.try_clone()?);
    let writer = file;

    Ok((reader, writer))
}

/// Stub for non-Windows platforms — named pipes are a Windows-only feature.
#[cfg(not(windows))]
fn create_and_wait_for_client(
    _pipe_name: &str,
) -> io::Result<(BufReader<std::fs::File>, std::fs::File)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Named pipes are Windows-only",
    ))
}

/// Handle a single JSON-RPC connection: read newline-delimited requests,
/// dispatch them, and write responses.
///
/// Also registers for server push events (e.g. `context_exited`) so that
/// backend clients are notified asynchronously when panes die.
fn handle_rpc_connection(
    reader: impl BufRead,
    writer: impl Write + Send + 'static,
    tx: mpsc::Sender<CtrlReq>,
) -> io::Result<()> {
    use super::dispatcher::dispatch_rpc;
    use std::sync::Arc;

    // Hold the writer behind a mutex so both the request/response loop and
    // the push-event writer thread can write to the same pipe connection.
    let writer = Arc::new(std::sync::Mutex::new(writer));
    let writer_for_events = Arc::clone(&writer);

    // Register for push events (e.g. context_exited notifications)
    let (event_tx, event_rx) = mpsc::channel::<String>();
    crate::types::register_backend_event_sender(event_tx);

    // Spawn a writer thread that forwards push events to the pipe
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

    // Request/response loop
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
