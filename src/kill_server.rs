//! `psmux kill-server`: end every server in scope, including a warm standby
//! that registers after the registry was read (see `warm_reap`).

use std::io::Write as _;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::warm_reap::{enumerate_registry, now_filetime, stamp_and_enumerate, KillScope};

/// End every server in namespace `ns` (`-L`), or every server everywhere when
/// `ns` is `None`.
///
/// The kill is stamped before the registry is read and again after, before
/// anything is ended; that ordering is what lets a standby the enumeration
/// missed find out it was killed (`warm_reap::warm_standby_verdict`).
pub fn kill_server(ns: Option<&str>) {
    let dir = PathBuf::from(crate::paths::psmux_dir());
    let scope = ns.map_or(KillScope::All, KillScope::Namespace);
    let ns_prefix = ns.map(|l| format!("{l}__"));
    let registered = stamp_and_enumerate(&dir, scope, ns_prefix.as_deref(), now_filetime);

    let streams: Vec<TcpStream> = registered
        .iter()
        .filter_map(|(path, contents)| request_kill(path, contents))
        .collect();
    for stream in streams {
        wait_for_close(stream);
    }
    // Only without -L: with -L, only that namespace's sessions may be touched.
    std::thread::sleep(Duration::from_millis(300));
    if ns.is_none() {
        crate::session::kill_remaining_server_processes();
        // The nuclear kill also ends a standby that registered after the
        // enumeration, before it could see the marker and unregister itself.
        // Read the registry again so its set is swept too; the sweep still
        // leaves any set whose port is bound.
        crate::session::sweep_dead_registry_sets(&enumerate_registry(&dir, None));
        return;
    }
    crate::session::sweep_dead_registry_sets(&registered);
}

/// Ask the server registered at `path` to exit. Returns the connection, whose
/// close signals that it did; `None` when there is nothing to connect to.
fn request_kill(path: &Path, contents: &str) -> Option<TcpStream> {
    let port = contents.trim().parse::<u16>().ok()?;
    let session = path.file_stem().and_then(|s| s.to_str())?;
    let key = crate::session::read_session_key(session).unwrap_or_default();
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(1000)).ok()?;
    let _ = stream.set_nodelay(true);
    let _ = writeln!(stream, "AUTH {}", key);
    let _ = stream.write_all(b"kill-server\n");
    let _ = stream.flush();
    // Half-close: done sending, but the read half detects the server's exit.
    let _ = stream.shutdown(std::net::Shutdown::Write);
    Some(stream)
}

/// Read until the server closes the connection (it has exited) or 3s pass.
fn wait_for_close(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(3000)));
    let mut buf = [0u8; 64];
    while let Ok(n) = std::io::Read::read(&mut stream, &mut buf) {
        if n == 0 {
            break;
        }
    }
}
