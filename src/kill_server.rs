//! `psmux kill-server`: end every server in scope, including a warm standby
//! that registers after the registry was read (see `warm_reap`).

use std::io::Write as _;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::warm_reap::{now_filetime, write_kill_marker, KillScope};

/// End every server in namespace `ns` (`-L`), or every server everywhere when
/// `ns` is `None`.
///
/// The kill is stamped before the registry is read and again after, before
/// anything is ended; that ordering is what lets a standby the enumeration
/// missed find out it was killed (`warm_reap::warm_standby_verdict`).
pub fn kill_server(ns: Option<&str>) {
    let dir = PathBuf::from(crate::paths::psmux_dir());
    let scope = ns.map_or(KillScope::All, KillScope::Namespace);
    write_kill_marker(&dir, scope, now_filetime());
    let ns_prefix = ns.map(|l| format!("{l}__"));
    let registered = enumerate_scope(&dir, ns_prefix.as_deref());
    write_kill_marker(&dir, scope, now_filetime());

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
    }
    crate::session::sweep_dead_registry_sets(&registered);
}

/// Every `.port` file in `dir` whose session falls under `ns_prefix` (all of
/// them when `None`), with its contents as read now.
fn enumerate_scope(dir: &Path, ns_prefix: Option<&str>) -> Vec<(PathBuf, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "port"))
        .filter(|p| {
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            ns_prefix.is_none_or(|pfx| stem.starts_with(pfx))
        })
        .filter_map(|p| std::fs::read_to_string(&p).ok().map(|c| (p, c)))
        .collect()
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
