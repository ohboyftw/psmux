use std::env;
use std::io::{self, Write};
use std::time::Duration;

/// Returns true if this port-file base name belongs to a warm (standby) server.
/// Warm sessions should be hidden from user-facing lists and never auto-attached.
/// Recognizes both the single-server name (`__warm__`) and pooled names
/// (`__warm__0`, `__warm__1`, ...), with or without a namespace prefix.
pub fn is_warm_session(base: &str) -> bool {
    // Extract the session name (strip namespace prefix if present)
    let session = if let Some(pos) = base.rfind("__") {
        // Check if this is a namespace prefix (e.g. "ns____warm__")
        // The namespace separator is `__` so find the session part
        let candidate = &base[pos + 2..];
        if candidate.is_empty() {
            // The `__` was at the end, so check from the start
            base
        } else {
            // Could be "ns____warm__" -> session part after last "__"
            // But we need to handle "ns____warm__0" too
            base
        }
    } else {
        base
    };
    // Direct match: `__warm__` or namespaced `ns____warm__`
    if session == "__warm__" || session.ends_with("____warm__") {
        return true;
    }
    // Pooled match: `__warm__N` or namespaced `ns____warm__N`
    // Check if base contains `__warm__` followed by only digits
    if let Some(pos) = session.find("__warm__") {
        let after = &session[pos + 8..]; // len("__warm__") == 8
        if !after.is_empty() && after.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    false
}

/// Find the next available numeric session name (tmux-compatible).
/// tmux uses a monotonically incrementing counter, but since psmux has
/// no persistent server state, we scan existing port files and pick
/// the lowest non-negative integer not already in use.
/// When `ns_prefix` is Some("foo"), names are checked as "foo__0", "foo__1", etc.
pub fn next_session_name(ns_prefix: Option<&str>) -> String {
    // No data directory (no home) means there is nothing to read; the same
    // early exit the home lookup used to provide.
    if crate::paths::psmux_dir_opt().is_none() {
        return "0".to_string();
    }
    let psmux_dir = crate::paths::psmux_dir();
    let mut used: std::collections::HashSet<u32> = std::collections::HashSet::new();
    if let Ok(entries) = std::fs::read_dir(&psmux_dir) {
        for entry in entries.flatten() {
            if let Some(fname) = entry.file_name().to_str() {
                if let Some((base, ext)) = fname.rsplit_once('.') {
                    if ext != "port" {
                        continue;
                    }
                    if is_warm_session(base) {
                        continue;
                    }
                    // Extract the session name part (after namespace prefix if any)
                    let session_part = if let Some(pfx) = ns_prefix {
                        let full_pfx = format!("{}__", pfx);
                        if base.starts_with(&full_pfx) {
                            &base[full_pfx.len()..]
                        } else {
                            continue; // different namespace
                        }
                    } else {
                        if base.contains("__") {
                            continue;
                        } // namespaced session
                        base
                    };
                    if let Ok(n) = session_part.parse::<u32>() {
                        used.insert(n);
                    }
                }
            }
        }
    }
    let mut id = 0u32;
    while used.contains(&id) {
        id += 1;
    }
    id.to_string()
}

/// Per-server files written alongside a `.port`. None may outlive their server.
const SERVER_STATE_EXTS: [&str; 3] = ["pipe", "key", "version"];

/// Clean up any stale port files (where server is not actually running)
pub fn cleanup_stale_port_files() {
    // No data directory (no home) means there is nothing to read; the same
    // early exit the home lookup used to provide.
    if crate::paths::psmux_dir_opt().is_none() {
        return;
    }
    let psmux_dir = crate::paths::psmux_dir();
    cleanup_stale_state_in(std::path::Path::new(&psmux_dir), SIDECAR_GRACE);
}

/// Remove per-server state files whose server is gone (issue #136).
///
/// Two passes are needed.  The first is keyed on `.port` files and reaches a
/// dead server's whole file set.  The second sweeps sidecars that have no
/// `.port` sibling at all — a graceful exit removes the `.port` *before* the
/// sidecars, and a killed server removes nothing, so any sidecar left behind is
/// invisible to the first pass and would accumulate forever.  That is how 160
/// `.pipe` files piled up against 6 `.port` files.
///
/// The second pass only removes sidecars older than `sidecar_grace`.  A server
/// writes its state files non-atomically, both at startup and during
/// [`migrate_server_state`], so a sidecar that is briefly unparented is normal
/// and must not be reaped — doing so cost a session its `.key` permanently.
fn cleanup_stale_state_in(dir: &std::path::Path, sidecar_grace: Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let paths: Vec<std::path::PathBuf> = entries.flatten().map(|e| e.path()).collect();

    for path in paths.iter().filter(|p| has_ext(p, "port")) {
        if !port_file_is_live(path) {
            remove_server_state(path);
        }
    }

    for path in paths
        .iter()
        .filter(|p| SERVER_STATE_EXTS.iter().any(|e| has_ext(p, e)))
    {
        if !path.with_extension("port").exists() && !is_within_grace(path, sidecar_grace) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Move a server's state files from `old_base` to `new_base`.
///
/// The ordering here is load-bearing. [`cleanup_stale_state_in`] removes
/// sidecars that have no `.port` sibling, so the old `.port` must outlive the
/// old sidecars, and the new `.port` must be written only once the new sidecars
/// are in place. Unlinking the old `.port` first — as the rename and claim paths
/// used to — let a concurrent psmux invocation delete the `.key` mid-move, after
/// which it was never re-written: the session ended up with a valid `.port` and
/// no credential, unauthenticatable for the rest of its life. The freshly
/// written sidecars are unparented until the new `.port` lands, which is what
/// `SIDECAR_GRACE` covers.
pub fn migrate_server_state(old_base: &str, new_base: &str, port: u16) {
    // No data directory (no home) means there is nothing to read; the same
    // early exit the home lookup used to provide.
    if crate::paths::psmux_dir_opt().is_none() {
        return;
    }
    let psmux_dir = crate::paths::psmux_dir();
    migrate_server_state_in(std::path::Path::new(&psmux_dir), old_base, new_base, port);
}

fn migrate_server_state_in(dir: &std::path::Path, old_base: &str, new_base: &str, port: u16) {
    // A same-name rename would otherwise write the new files and then delete
    // them again as "the old ones".
    if old_base == new_base {
        return;
    }
    let old = |ext: &str| dir.join(format!("{}.{}", old_base, ext));
    let new = |ext: &str| dir.join(format!("{}.{}", new_base, ext));

    // Carry the credential across before anything is unlinked.
    if let Ok(key) = std::fs::read_to_string(old("key")) {
        let _ = std::fs::write(new("key"), key);
    }
    let _ = std::fs::write(new("version"), crate::types::build_version_stamp());
    if old("pipe").exists() {
        let _ = std::fs::rename(old("pipe"), new("pipe"));
    }

    // `.port` last: it is the readiness beacon, and it is what parents the new
    // sidecars against the orphan sweep.
    let _ = std::fs::write(new("port"), port.to_string());

    for ext in SERVER_STATE_EXTS {
        let _ = std::fs::remove_file(old(ext));
    }
    let _ = std::fs::remove_file(old("port"));
}

fn has_ext(path: &std::path::Path, ext: &str) -> bool {
    path.extension().map(|e| e == ext).unwrap_or(false)
}

/// A sidecar younger than this is left alone even with no `.port` sibling.
/// Servers write their state files non-atomically — during startup and during
/// [`migrate_server_state`] a sidecar legitimately exists before its `.port`
/// does — so without a grace period the sweep races every server that is
/// starting or being renamed.
const SIDECAR_GRACE: Duration = Duration::from_secs(60);

/// True when `path`'s age cannot be established or is under `grace`.
/// Unknown age counts as young: the sweep must never delete on a guess.
fn is_within_grace(path: &std::path::Path, grace: Duration) -> bool {
    let Ok(modified) = std::fs::metadata(path).and_then(|m| m.modified()) else {
        return true;
    };
    modified.elapsed().map(|age| age < grace).unwrap_or(true)
}

/// A `.port` file is live when its port is still claimed by some process.
/// An unreadable file is left alone; an unparseable one counts as dead.
///
/// Liveness is decided by trying to *bind* the port, not by connecting to it.
/// Connecting cannot answer the question quickly on Windows: a loopback port
/// that nothing is listening on takes ~2s of SYN retransmits before it reports
/// `ConnectionRefused`, and every budget shorter than that returns `TimedOut`
/// instead — measured on this machine at 50ms/250ms/500ms/1s. The previous
/// 50ms-connect probe therefore read *every* verdict as "dead", including a
/// live server too busy to complete a handshake, and would have reaped a
/// running session's whole file set. Binding settles it in tens of microseconds
/// and cannot be confused by load: a bound socket stays bound for the life of
/// the server process, whether or not anyone is calling `accept`.
///
/// A bind that fails for any other reason (a port inside a Windows excluded
/// range, another process holding it) counts as live — the sweep must never
/// delete on a guess. Servers bind port 0 and let the OS choose, so this never
/// races a server trying to claim a specific port.
fn port_file_is_live(path: &std::path::Path) -> bool {
    let Ok(port_str) = std::fs::read_to_string(path) else {
        return true;
    };
    let Ok(port) = port_str.trim().parse::<u16>() else {
        return false;
    };
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

fn remove_server_state(port_path: &std::path::Path) {
    let _ = std::fs::remove_file(port_path);
    for ext in SERVER_STATE_EXTS {
        let _ = std::fs::remove_file(port_path.with_extension(ext));
    }
}

/// Kill any warm (standby) servers in the given namespace.
/// Called when the last non-warm session exits so orphan warm servers don't
/// linger indefinitely (#120, #138).  Skips if other non-warm sessions still
/// exist (warm servers may be needed for `new-session`).
pub fn kill_warm_servers(ns_prefix: Option<&str>) {
    // No data directory (no home) means there is nothing to read; the same
    // early exit the home lookup used to provide.
    if crate::paths::psmux_dir_opt().is_none() {
        return;
    }
    let psmux_dir = crate::paths::psmux_dir();
    let entries = match std::fs::read_dir(&psmux_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut has_non_warm = false;
    let mut warm_targets: Vec<(String, u16)> = Vec::new(); // (session_name, port)

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "port").unwrap_or(false) {
            if let Some(session_name) = path.file_stem().and_then(|s| s.to_str()) {
                // Apply namespace filtering
                if let Some(pfx) = ns_prefix {
                    if !session_name.starts_with(pfx) {
                        continue;
                    }
                }
                if is_warm_session(session_name) {
                    if let Ok(port_str) = std::fs::read_to_string(&path) {
                        if let Ok(port) = port_str.trim().parse::<u16>() {
                            warm_targets.push((session_name.to_string(), port));
                        }
                    }
                } else {
                    // Another non-warm session exists — warm servers are still needed
                    has_non_warm = true;
                }
            }
        }
    }

    if has_non_warm {
        return; // Other sessions still running, keep warm servers alive
    }

    // No other non-warm sessions — kill all warm servers
    for (session_name, port) in warm_targets {
        let key = read_session_key(&session_name).unwrap_or_default();
        let addr = format!("127.0.0.1:{}", port);
        let _ = send_auth_cmd(&addr, &key, b"kill-server\n");
    }
}

/// Read the session key from the key file
pub fn read_session_key(session: &str) -> io::Result<String> {
    let keypath = crate::paths::key_file(session);
    std::fs::read_to_string(&keypath).map(|s| s.trim().to_string())
}

/// Send an authenticated command to a server
pub fn send_auth_cmd(addr: &str, key: &str, cmd: &[u8]) -> io::Result<()> {
    let sock_addr: std::net::SocketAddr = addr
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    if let Ok(mut s) = std::net::TcpStream::connect_timeout(&sock_addr, Duration::from_millis(50)) {
        let _ = s.set_nodelay(true);
        let _ = writeln!(s, "AUTH {}", key);
        let _ = std::io::Write::write_all(&mut s, cmd);
        let _ = s.flush();
    }
    Ok(())
}

/// Send an authenticated command and get response
pub fn send_auth_cmd_response(addr: &str, key: &str, cmd: &[u8]) -> io::Result<String> {
    let mut s = std::net::TcpStream::connect(addr)?;
    let _ = s.set_nodelay(true);
    let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = writeln!(s, "AUTH {}", key);
    let _ = std::io::Write::write_all(&mut s, cmd);
    let _ = s.flush();
    let mut br = std::io::BufReader::new(&mut s);
    let mut auth_line = String::new();
    let _ = std::io::BufRead::read_line(&mut br, &mut auth_line);
    let mut buf = String::new();
    let _ = std::io::Read::read_to_string(&mut br, &mut buf);
    Ok(buf)
}

/// Parse `-t <target>` from a control command line.
/// Handles `-t FOO`, `-t "FOO"`, and targets with `:window[.pane]` suffix.
fn parse_target_from_line(line: &str) -> Option<String> {
    let mut iter = line.split_whitespace();
    while let Some(tok) = iter.next() {
        if tok == "-t" {
            if let Some(val) = iter.next() {
                return Some(val.trim_matches('"').to_string());
            }
        }
    }
    None
}

/// Ask a server whether it owns the pane-id or window-id. Returns true on match.
/// Short per-connection timeouts keep scans cheap (< 50 ms total per server).
fn server_owns_id(port: u16, key: &str, target_id: &str) -> bool {
    let debug = env::var("PSMUX_DEBUG_ROUTING").is_ok();
    let addr_s = format!("127.0.0.1:{}", port);
    let Ok(addr) = addr_s.parse::<std::net::SocketAddr>() else {
        return false;
    };
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(300))
    else {
        if debug {
            eprintln!("[route] connect {}:{} failed", addr_s, port);
        }
        return false;
    };
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = writeln!(stream, "AUTH {}", key);
    let query = if target_id.starts_with('%') {
        "list-panes -a -F #{pane_id}\n"
    } else {
        "list-windows -a -F #{window_id}\n"
    };
    let _ = write!(stream, "{}", query);
    let _ = stream.flush();
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    loop {
        match std::io::Read::read(&mut stream, &mut tmp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let owns = text.lines().any(|l| l.trim() == target_id);
    if debug {
        eprintln!(
            "[route] port={} target={} owns={} resp_bytes={} resp={:?}",
            port,
            target_id,
            owns,
            buf.len(),
            text
        );
    }
    owns
}

/// Scan every live `.port` file and find which server owns the pane-id/window-id.
/// Iterates in reverse-mtime order (newest first) so fresh sessions win over
/// long-running terminals that happen to have a pane with the same numeric id.
/// Reserve a block of pane ids for this server; returns the first id to hand out.
///
/// Every `AppState` used to start at 1, so concurrently live servers all minted
/// `%1, %2, ...`. A bare `-t %N` is resolved by scanning live servers (biased
/// toward the caller's own session), so colliding ids let a command aimed at one
/// session land in another — `respawn-pane -k` then kills the wrong pane, and
/// with `remain-on-exit=off` a single-pane session is pruned along with it.
///
/// Ids only have to be unambiguous among *live* servers, and a shared monotonic
/// counter guarantees that. A block of `block` ids is taken per server start, so
/// this bounds a single server to that many panes over its lifetime.
pub fn reserve_pane_id_base(block: usize) -> usize {
    let dir = crate::paths::psmux_dir();
    let _ = std::fs::create_dir_all(&dir);
    let seq_path = format!("{}\\pane_id_seq", dir);
    let lock_path = format!("{}\\pane_id_seq.lock", dir);
    for attempt in 0..50 {
        if std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .is_ok()
        {
            let cur = std::fs::read_to_string(&seq_path)
                .ok()
                .and_then(|s| s.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let _ = std::fs::write(&seq_path, (cur + block).to_string());
            let _ = std::fs::remove_file(&lock_path);
            return cur + 1;
        }
        // Clear a lock orphaned by a crashed server rather than spin forever.
        if attempt == 25 {
            let stale = std::fs::metadata(&lock_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|m| m.elapsed().ok())
                .is_some_and(|e| e > Duration::from_secs(5));
            if stale {
                let _ = std::fs::remove_file(&lock_path);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    1
}

fn scan_servers_for_id(target_id: &str) -> Option<(String, u16, String)> {
    // Bias toward the caller's own server when scripting from inside a psmux
    // pane: PSMUX_SESSION names the server that owns this process, so pane-ids
    // it mints are guaranteed to belong to it (unambiguous). Fall back to
    // reverse-mtime across all live servers otherwise.
    let own_session = env::var("PSMUX_SESSION")
        .ok()
        .filter(|s| !s.is_empty() && !is_warm_session(s));
    let dir = crate::paths::psmux_dir();
    let mut candidates: Vec<(String, std::time::SystemTime)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let fname = entry.file_name();
            let fname = fname.to_string_lossy();
            let Some(name) = fname.strip_suffix(".port") else {
                continue;
            };
            if is_warm_session(name) {
                continue;
            }
            let mtime = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            candidates.push((name.to_string(), mtime));
        }
    }
    candidates.sort_by(|a, b| {
        // Own session wins outright, regardless of mtime.
        let a_own = own_session.as_deref() == Some(a.0.as_str());
        let b_own = own_session.as_deref() == Some(b.0.as_str());
        b_own.cmp(&a_own).then_with(|| b.1.cmp(&a.1))
    });
    for (name, _) in candidates {
        let path = crate::paths::port_file(&name);
        let Some(port) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse::<u16>().ok())
        else {
            continue;
        };
        let key = read_session_key(&name).unwrap_or_default();
        if server_owns_id(port, &key, target_id) {
            return Some((name, port, key));
        }
    }
    None
}

/// Resolve which server (name, port, session-key) a control command should be
/// sent to. Priority: (1) `-t <target>` parsed from the line — routes a session
/// name to `<name>.port` directly, and scans live servers for `%pane` / `@window`
/// ids. (2) `PSMUX_TARGET_SESSION` env var. (3) `resolve_last_session_name`
/// fallback when env is unset or points to a warm server.
fn resolve_server_for_command(line: &str) -> io::Result<(String, u16, String)> {
    let debug = env::var("PSMUX_DEBUG_ROUTING").is_ok();
    if debug {
        eprintln!("[route] resolve for line={:?}", line);
    }
    // (1) target hint from `-t`
    if let Some(raw) = parse_target_from_line(line) {
        if debug {
            eprintln!("[route] -t target parsed: {:?}", raw);
        }
        if raw.starts_with('%') || raw.starts_with('@') {
            if debug {
                eprintln!("[route] scanning servers for id {}", raw);
            }
            if let Some(r) = scan_servers_for_id(&raw) {
                if debug {
                    eprintln!("[route] scan hit: session={} port={}", r.0, r.1);
                }
                return Ok(r);
            }
            if debug {
                eprintln!("[route] scan miss");
            }
            // fall through — no live server owns that id; use env fallback so
            // the server's own error message (or downstream command) surfaces.
        } else {
            // Session name; strip `:window[.pane]` suffix.
            let name = raw.split(':').next().unwrap_or(&raw);
            if !name.is_empty() && !is_warm_session(name) {
                let path = crate::paths::port_file(name);
                if let Some(port) = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|s| s.trim().parse::<u16>().ok())
                {
                    let key = read_session_key(name).unwrap_or_default();
                    return Ok((name.to_string(), port, key));
                }
            }
        }
    }
    // (2,3) env + warm fallback — preserves historical behaviour.
    let mut target = env::var("PSMUX_TARGET_SESSION")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".to_string());
    if debug {
        eprintln!("[route] env fallback: PSMUX_TARGET_SESSION -> {:?}", target);
    }
    if is_warm_session(&target) {
        target = resolve_last_session_name().unwrap_or_else(|| "default".to_string());
        if debug {
            eprintln!("[route] warm resolved to {:?}", target);
        }
    }
    let path = crate::paths::port_file(&target);
    let port = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .ok_or_else(|| io::Error::other(format!("no server running on session '{}'", target)))?;
    let key = read_session_key(&target).unwrap_or_default();
    if debug {
        eprintln!("[route] env path: session={} port={}", target, port);
    }
    Ok((target, port, key))
}

pub fn send_control(line: String) -> io::Result<()> {
    let (_target, port, session_key) = resolve_server_for_command(&line)?;
    let full_target = env::var("PSMUX_TARGET_FULL").ok();
    let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
    let mut stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(100))?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
    let _ = writeln!(stream, "AUTH {}", session_key);
    if let Some(ref ft) = full_target {
        let _ = writeln!(stream, "TARGET {}", ft);
    }
    let _ = write!(stream, "{}", line);
    let _ = stream.flush();
    // Read the "OK" response to drain the receive buffer before closing.
    // This prevents Windows from sending RST (due to unread data) which
    // could cause the server to lose the command.
    let mut buf = [0u8; 64];
    let _ = std::io::Read::read(&mut stream, &mut buf);
    Ok(())
}

/// True when a reply is the server refusing the connection outright, rather
/// than command output.
///
/// `send_control_with_response` is the chokepoint every one-shot CLI read verb
/// goes through, and it classified only the AUTH ack — so a refusal came back
/// as reply DATA, was printed to stdout, and the process exited 0. A machine
/// consumer parsing `list-windows` ingested `ERROR: Authentication required`
/// as a window record, and no script or CI gate could detect the failure by
/// exit code (#561).
///
/// This matches the two exact server strings as WHOLE payloads, deliberately
/// not an `ERROR:` prefix: `capture-pane` and `show-buffer` return arbitrary
/// pane content that may legitimately begin with the word ERROR, and
/// misreading that as a refusal would be a worse bug than the one being fixed.
/// The AUTH ack must be stripped before calling this — it is protocol framing,
/// not payload.
pub(crate) fn is_server_refusal(payload: &str) -> bool {
    matches!(
        payload.trim_end_matches(['\r', '\n']),
        "ERROR: Authentication required" | "ERROR: Invalid session key"
    )
}

/// True when a reply is the server reporting that the `-t` target does not
/// resolve, rather than command output.
///
/// Same whole-payload discipline as [`is_server_refusal`], and the same
/// accepted residual: a `capture-pane` of a pane whose entire content is
/// exactly one of these lines would be misread as an error. That is strictly
/// better than what it replaces — a typo'd `-t` printing to stdout at rc 0,
/// which is indistinguishable from the command having worked.
pub(crate) fn is_unresolved_target(payload: &str) -> bool {
    let p = payload.trim_end_matches(['\r', '\n']);
    !p.contains('\n')
        && (p.starts_with(crate::types::UNRESOLVED_PANE_PREFIX)
            || p.starts_with(crate::types::UNRESOLVED_WINDOW_PREFIX))
}

/// A bare `TcpStream::connect` to a port nothing answers on can hang for the
/// full Windows SYN-retransmit schedule (~21s) before failing. A live server is
/// on loopback and completes the handshake in microseconds, so the only thing
/// this budget has to accommodate is how long a *dead* port takes to say so.
///
/// **Do not lower this.** Per the measurement in `port_file_is_live` above
/// (see the doc comment at the top of that fn), an unbound loopback port on
/// Windows needs ~2s of SYN retransmits before it reports `ConnectionRefused`,
/// and every budget shorter than that returns `TimedOut` instead — measured at
/// 50ms/250ms/500ms/1s. Anything under ~2s therefore reports "the server
/// stalled" when the truth is "there is no server", which is exactly the
/// confusion the rest of this function exists to remove.
const CONTROL_CONNECT_TIMEOUT: Duration = Duration::from_millis(3000);

/// Read timeout for a one-shot control reply. Matches upstream; the extra
/// second over the old 2000ms is headroom for server-side round trips that
/// happen before the reply is written.
const CONTROL_READ_TIMEOUT: Duration = Duration::from_millis(3000);

/// Drain `stream` until EOF, reporting whether the read stopped at a timeout
/// rather than at end-of-reply.
///
/// The bool is the whole point: a stalled server and a finished one used to be
/// indistinguishable here, because both left the loop through `break`.
fn read_until_eof(stream: &mut std::net::TcpStream) -> (Vec<u8>, bool) {
    let mut buf = Vec::new();
    let mut temp = [0u8; 4096];
    loop {
        match std::io::Read::read(stream, &mut temp) {
            Ok(0) => return (buf, false),
            Ok(n) => buf.extend_from_slice(&temp[..n]),
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                return (buf, true)
            }
            Err(_) => return (buf, false),
        }
    }
}

/// Write one request on an authenticated one-shot control socket and return the
/// server's reply.
///
/// `stream` must already carry its read timeout: the caller owns that policy
/// (`wait-pane` blocks for minutes, `list-windows` for milliseconds).
fn exchange_one_shot(
    stream: &mut std::net::TcpStream,
    session_key: &str,
    full_target: Option<&str>,
    line: &str,
) -> io::Result<String> {
    let _ = writeln!(stream, "AUTH {}", session_key);
    if let Some(ft) = full_target {
        let _ = writeln!(stream, "TARGET {}", ft);
    }
    let _ = write!(stream, "{}", line);
    let _ = stream.flush();
    // Half-close so the server's `read_line` sees EOF right after our request
    // (server/connection.rs:225-230) and closes as soon as the reply is
    // written. That makes end-of-reply a definitive `Ok(0)` instead of an
    // idle-gap guess, which is what lets a timeout below mean "stalled"
    // unambiguously. Precedent in this codebase: main.rs:318-320.
    let _ = stream.shutdown(std::net::Shutdown::Write);

    let (buf, timed_out) = read_until_eof(stream);
    let result = String::from_utf8_lossy(&buf).to_string();
    // Strip the "OK\n" AUTH response prefix if present
    let result = if let Some(rest) = result.strip_prefix("OK\n") {
        rest.to_string()
    } else if let Some(rest) = result.strip_prefix("OK\r\n") {
        rest.to_string()
    } else {
        result
    };
    // A refusal is not command output: without this it was printed to stdout at
    // rc 0, so no script, orchestrate step or CI gate could detect an auth
    // failure by exit code (#561). It is classified BEFORE the stall check, so
    // a server that refuses and then holds the socket still reports the refusal
    // — the specific diagnosis outranks the generic one.
    if is_server_refusal(&result) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            result.trim_end().to_string(),
        ));
    }
    // An unresolvable `-t` is a failed command, not output. Printed to stdout
    // at rc 0 it read as success, so a script targeting a pane that had already
    // died carried on as though it had written to it (#545).
    if is_unresolved_target(&result) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            result.trim_end().to_string(),
        ));
    }
    // With the half-close in place the server ends every one-shot reply with a
    // close, so a timeout is never "the reply just ended" — it is a stall or a
    // truncation, and returning Ok here made both look like success (#561).
    if timed_out {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "incomplete reply from server ({} bytes before stall)",
                buf.len()
            ),
        ));
    }
    Ok(result)
}

pub fn send_control_with_response(line: String) -> io::Result<String> {
    let (_target, port, session_key) = resolve_server_for_command(&line)?;
    let full_target = env::var("PSMUX_TARGET_FULL").ok();
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = std::net::TcpStream::connect_timeout(&addr, CONTROL_CONNECT_TIMEOUT)?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(CONTROL_READ_TIMEOUT));
    exchange_one_shot(&mut stream, &session_key, full_target.as_deref(), &line)
}

/// Send a control message and wait for the full response with a custom timeout.
///
/// This is like `send_control_with_response` but allows specifying a longer
/// read timeout, which is needed for blocking commands like `wait-pane` that
/// may take many seconds (or minutes) to complete.
///
/// Pass `None` for `timeout` to block indefinitely (no read timeout) — that
/// variant can never report a stall, which is the intended wait-pane semantics.
pub fn send_control_with_response_timeout(
    line: String,
    timeout: Option<Duration>,
) -> io::Result<String> {
    let (_target, port, session_key) = resolve_server_for_command(&line)?;
    let full_target = env::var("PSMUX_TARGET_FULL").ok();
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = std::net::TcpStream::connect_timeout(&addr, CONTROL_CONNECT_TIMEOUT)?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(timeout);
    exchange_one_shot(&mut stream, &session_key, full_target.as_deref(), &line)
}

/// Send a control message to a specific port with authentication
pub fn send_control_to_port(port: u16, msg: &str, session_key: &str) -> io::Result<()> {
    let addr = format!("127.0.0.1:{}", port);
    if let Ok(mut stream) = std::net::TcpStream::connect(&addr) {
        let _ = stream.set_nodelay(true);
        let _ = writeln!(stream, "AUTH {}", session_key);
        let _ = stream.write_all(msg.as_bytes());
        let _ = stream.flush();
        // Drain the OK response to prevent RST
        let mut buf = [0u8; 64];
        let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
        let _ = std::io::Read::read(&mut stream, &mut buf);
    }
    Ok(())
}

pub fn resolve_last_session_name() -> Option<String> {
    crate::paths::psmux_dir_opt()?;
    let dir = crate::paths::psmux_dir();
    let last = std::fs::read_to_string(format!("{}\\last_session", dir)).ok();
    if let Some(name) = last {
        let name = name.trim().to_string();
        let p = format!("{}\\{}.port", dir, name);
        if std::path::Path::new(&p).exists() {
            return Some(name);
        }
    }
    let mut picks: Vec<(String, std::time::SystemTime)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if let Some(fname) = e.file_name().to_str() {
                if let Some((base, ext)) = fname.rsplit_once('.') {
                    if ext == "port" {
                        if let Ok(md) = e.metadata() {
                            picks.push((
                                base.to_string(),
                                md.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                            ));
                        }
                    }
                }
            }
        }
    }
    // Exclude warm (standby) sessions — users should never auto-attach to them
    picks.retain(|(n, _)| !is_warm_session(n));
    picks.sort_by_key(|(_, t)| *t);
    picks.last().map(|(n, _)| n.clone())
}

pub fn resolve_default_session_name() -> Option<String> {
    if let Ok(name) = env::var("PSMUX_DEFAULT_SESSION") {
        crate::paths::psmux_dir_opt()?;
        let p = crate::paths::port_file(&name);
        if std::path::Path::new(&p).exists() {
            return Some(name);
        }
    }
    // .psmuxrc is a dotfile in HOME; pmuxrc lives in the data directory.
    let home = crate::paths::home_dir();
    if home.is_empty() {
        return None;
    }
    let candidates = [
        format!("{}\\.psmuxrc", home),
        crate::paths::psmux_dir_file("pmuxrc"),
    ];
    for cfg in candidates.iter() {
        if let Ok(text) = std::fs::read_to_string(cfg) {
            let line = text.lines().find(|l| !l.trim().is_empty())?;
            let name = if let Some(rest) = line.strip_prefix("default-session ") {
                rest.trim().to_string()
            } else {
                line.trim().to_string()
            };
            let p = crate::paths::port_file(&name);
            if std::path::Path::new(&p).exists() {
                return Some(name);
            }
        }
    }
    None
}

pub fn reap_children_placeholder() -> io::Result<bool> {
    Ok(false)
}

/// Return the names of all live sessions by scanning .psmux/*.port files.
pub fn list_session_names() -> Vec<String> {
    let dir = crate::paths::psmux_dir();
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if let Some(fname) = e.file_name().to_str().map(|s| s.to_string()) {
                if let Some((base, ext)) = fname.rsplit_once('.') {
                    if ext == "port" {
                        if is_warm_session(base) {
                            continue;
                        }
                        names.push(base.to_string());
                    }
                }
            }
        }
    }
    names.sort();
    names
}

/// A tree entry used by choose-tree: either a session header or a window under a session.
#[derive(Clone, Debug)]
pub struct TreeEntry {
    pub session_name: String,
    pub session_port: u16,
    pub is_session_header: bool,
    pub window_index: Option<usize>,
    pub window_name: String,
    pub window_panes: usize,
    pub window_size: String,
    pub is_current_session: bool,
    pub is_active_window: bool,
}

/// List all running sessions and their windows for choose-tree display.
/// Queries each running server via its TCP port for window list info.
pub fn list_all_sessions_tree(
    current_session: &str,
    current_windows: &[(String, usize, String, bool)],
) -> Vec<TreeEntry> {
    // No data directory (no home) means there is nothing to read; the same
    // early exit the home lookup used to provide.
    if crate::paths::psmux_dir_opt().is_none() {
        return vec![];
    }
    let psmux_dir = crate::paths::psmux_dir();
    let mut sessions: Vec<(String, u16, std::time::SystemTime)> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&psmux_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "port").unwrap_or(false) {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    // Hide warm (standby) sessions from choose-tree
                    if is_warm_session(stem) {
                        continue;
                    }
                    if let Ok(port_str) = std::fs::read_to_string(&path) {
                        if let Ok(port) = port_str.trim().parse::<u16>() {
                            let mtime = entry
                                .metadata()
                                .and_then(|m| m.modified())
                                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                            sessions.push((stem.to_string(), port, mtime));
                        }
                    }
                }
            }
        }
    }

    sessions.sort_by_key(|(name, _, _)| name.clone());

    let mut tree = Vec::new();
    for (name, port, _) in &sessions {
        let is_current = name == current_session;
        // Session header
        tree.push(TreeEntry {
            session_name: name.clone(),
            session_port: *port,
            is_session_header: true,
            window_index: None,
            window_name: String::new(),
            window_panes: 0,
            window_size: String::new(),
            is_current_session: is_current,
            is_active_window: false,
        });

        if is_current {
            // Use local data for the current session (fast, no IPC)
            for (i, (wname, panes, size, is_active)) in current_windows.iter().enumerate() {
                tree.push(TreeEntry {
                    session_name: name.clone(),
                    session_port: *port,
                    is_session_header: false,
                    window_index: Some(i),
                    window_name: wname.clone(),
                    window_panes: *panes,
                    window_size: size.clone(),
                    is_current_session: true,
                    is_active_window: *is_active,
                });
            }
        } else {
            // Query remote session for its window list
            let key = read_session_key(name).unwrap_or_default();
            let addr = format!("127.0.0.1:{}", port);
            if let Ok(resp) = send_auth_cmd_response(&addr, &key, b"list-windows -F \"#{window_index}:#{window_name}:#{window_panes}:#{window_width}x#{window_height}:#{window_active}\"\n") {
                for line in resp.lines() {
                    let line = line.trim();
                    if line.is_empty() { continue; }
                    let parts: Vec<&str> = line.splitn(5, ':').collect();
                    if parts.len() >= 5 {
                        let wi = parts[0].parse::<usize>().unwrap_or(0);
                        let wn = parts[1].to_string();
                        let wp = parts[2].parse::<usize>().unwrap_or(1);
                        let ws = parts[3].to_string();
                        let wa = parts[4] == "1";
                        tree.push(TreeEntry {
                            session_name: name.clone(),
                            session_port: *port,
                            is_session_header: false,
                            window_index: Some(wi),
                            window_name: wn,
                            window_panes: wp,
                            window_size: ws,
                            is_current_session: false,
                            is_active_window: wa,
                        });
                    }
                }
            }
        }
    }
    tree
}

/// Force-kill any remaining psmux/pmux/tmux server processes that didn't
/// exit via the TCP kill-server command.  This is the nuclear fallback that
/// guarantees kill-server always succeeds.
///
/// On Windows, uses CreateToolhelp32Snapshot to enumerate processes and
/// TerminateProcess to kill them.  Skips the current process.
#[cfg(windows)]
pub fn kill_remaining_server_processes() {
    const TH32CS_SNAPPROCESS: u32 = 0x00000002;
    const PROCESS_TERMINATE: u32 = 0x0001;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const INVALID_HANDLE: isize = -1;

    #[repr(C)]
    struct PROCESSENTRY32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(dw_flags: u32, th32_process_id: u32) -> isize;
        fn Process32FirstW(h_snapshot: isize, lppe: *mut PROCESSENTRY32W) -> i32;
        fn Process32NextW(h_snapshot: isize, lppe: *mut PROCESSENTRY32W) -> i32;
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> isize;
        fn TerminateProcess(h_process: isize, exit_code: u32) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }

    let my_pid = std::process::id();

    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE || snap == 0 {
            return;
        }

        let mut pe: PROCESSENTRY32W = std::mem::zeroed();
        pe.dw_size = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        let target_names: &[&str] = &["psmux.exe", "pmux.exe", "tmux.exe"];
        let mut pids_to_kill: Vec<u32> = Vec::new();

        if Process32FirstW(snap, &mut pe) != 0 {
            loop {
                let pid = pe.th32_process_id;
                if pid != my_pid {
                    // Extract exe name from wide string
                    let len = pe.sz_exe_file.iter().position(|&c| c == 0).unwrap_or(260);
                    let name = String::from_utf16_lossy(&pe.sz_exe_file[..len]);
                    let name_lower = name.to_lowercase();
                    for target in target_names {
                        if name_lower == *target || name_lower.ends_with(&format!("\\{}", target)) {
                            pids_to_kill.push(pid);
                            break;
                        }
                    }
                }
                if Process32NextW(snap, &mut pe) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);

        for pid in &pids_to_kill {
            let h = OpenProcess(
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                *pid,
            );
            if h != 0 && h != INVALID_HANDLE {
                let _ = TerminateProcess(h, 1);
                CloseHandle(h);
            }
        }
    }
}

#[cfg(not(windows))]
pub fn kill_remaining_server_processes() {
    // On non-Windows, use signal-based killing
    let _ = std::process::Command::new("pkill")
        .args(&["-f", "psmux|pmux"])
        .status();
}

#[cfg(test)]
mod test_stale_state_cleanup {
    use super::*;
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};

    fn fresh_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("clean pre-existing temp dir");
        }
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, contents).expect("write fixture");
        p
    }

    #[test]
    fn removes_sidecar_with_no_port_sibling() {
        let dir = fresh_dir("psmux_cleanup_orphan_sidecar");
        let pipe = write(&dir, "ghost.pipe", r"\.\pipe\psmux-ghost");

        cleanup_stale_state_in(&dir, Duration::ZERO);

        assert!(!pipe.exists(), "orphan .pipe should be removed");
    }

    /// The listener never calls `accept`, which is the point: a server too busy
    /// to answer must still read as live.
    #[test]
    fn keeps_sidecar_whose_port_is_still_bound() {
        let dir = fresh_dir("psmux_cleanup_live_sidecar");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind probe listener");
        let port = listener.local_addr().expect("local_addr").port();
        write(&dir, "live.port", &port.to_string());
        let pipe = write(&dir, "live.pipe", r"\.\pipe\psmux-live");

        cleanup_stale_state_in(&dir, Duration::ZERO);

        assert!(pipe.exists(), "sidecar of a live server must survive");
    }

    #[test]
    fn removes_every_sidecar_when_port_is_dead() {
        let dir = fresh_dir("psmux_cleanup_dead_server");
        // Port 1 is not bound by anything in the test environment.
        let port = write(&dir, "dead.port", "1");
        let pipe = write(&dir, "dead.pipe", r"\.\pipe\psmux-dead");
        let key = write(&dir, "dead.key", "deadbeef");
        let version = write(&dir, "dead.version", "3.4.0");

        cleanup_stale_state_in(&dir, Duration::ZERO);

        assert!(!port.exists(), ".port should be removed");
        assert!(!pipe.exists(), ".pipe should be removed");
        assert!(!key.exists(), ".key should be removed");
        assert!(!version.exists(), ".version should be removed");
    }

    #[test]
    fn treats_an_unparseable_port_file_as_dead() {
        let dir = fresh_dir("psmux_cleanup_garbage_port");
        let port = write(&dir, "garbage.port", "not-a-port");
        let key = write(&dir, "garbage.key", "deadbeef");

        cleanup_stale_state_in(&dir, Duration::ZERO);

        assert!(!port.exists(), "unparseable .port should be removed");
        assert!(!key.exists(), "its sidecars should go with it");
    }

    #[test]
    fn treats_an_empty_port_file_as_dead() {
        let dir = fresh_dir("psmux_cleanup_empty_port");
        let port = write(&dir, "empty.port", "");

        cleanup_stale_state_in(&dir, Duration::ZERO);

        assert!(
            !port.exists(),
            "empty .port names no port, so nothing is live"
        );
    }

    #[test]
    fn treats_a_port_above_the_u16_range_as_dead() {
        let dir = fresh_dir("psmux_cleanup_oversized_port");
        let port = write(&dir, "oversized.port", "70000");

        cleanup_stale_state_in(&dir, Duration::ZERO);

        assert!(
            !port.exists(),
            "70000 is not addressable, so nothing is live"
        );
    }

    /// Port 0 is the "let the OS choose" sentinel: binding it always succeeds,
    /// which must read as dead rather than as a successful liveness probe.
    #[test]
    fn treats_port_zero_as_dead() {
        let dir = fresh_dir("psmux_cleanup_port_zero");
        let port = write(&dir, "zero.port", "0");
        let key = write(&dir, "zero.key", "deadbeef");

        cleanup_stale_state_in(&dir, Duration::ZERO);

        assert!(!port.exists(), "a .port of 0 names no listening server");
        assert!(!key.exists(), "its sidecars should go with it");
    }

    #[test]
    fn reads_a_port_written_with_a_trailing_newline() {
        let dir = fresh_dir("psmux_cleanup_trailing_newline");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind probe listener");
        let port = listener.local_addr().expect("local_addr").port();
        write(&dir, "nl.port", &format!("{}\r\n", port));
        let key = write(&dir, "nl.key", "deadbeef");

        cleanup_stale_state_in(&dir, Duration::ZERO);

        assert!(
            key.exists(),
            "surrounding whitespace must not fake a dead server"
        );
    }

    /// An unreadable `.port` yields no verdict, and the sweep never deletes on a
    /// guess. A directory is the portable way to make `read_to_string` fail.
    #[test]
    fn spares_a_server_whose_port_file_cannot_be_read() {
        let dir = fresh_dir("psmux_cleanup_unreadable_port");
        let port = dir.join("locked.port");
        std::fs::create_dir(&port).expect("create unreadable .port");
        let key = write(&dir, "locked.key", "deadbeef");

        cleanup_stale_state_in(&dir, Duration::ZERO);

        assert!(port.exists(), "an unreadable .port must be left alone");
        assert!(key.exists(), "and so must its sidecars");
    }

    #[test]
    fn preserves_files_that_are_not_per_server_state() {
        let dir = fresh_dir("psmux_cleanup_preserves_globals");
        let seq = write(&dir, "pane_id_seq", "170000");
        let last = write(&dir, "last_session", "work");
        let log = write(&dir, "autorename.log", "noise");

        cleanup_stale_state_in(&dir, Duration::ZERO);

        assert!(seq.exists(), "pane_id_seq is global state, not per-server");
        assert!(
            last.exists(),
            "last_session is global state, not per-server"
        );
        assert!(log.exists(), "logs are not per-server state");
    }
}

#[cfg(test)]
mod test_state_migration {
    use super::*;
    use std::path::{Path, PathBuf};

    fn fresh_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("clean pre-existing temp dir");
        }
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn seed_server(dir: &Path, base: &str) {
        for (ext, body) in [("port", "5000"), ("key", "s3cret"), ("version", "3.4.0")] {
            std::fs::write(dir.join(format!("{}.{}", base, ext)), body).expect("seed");
        }
    }

    fn read(dir: &Path, name: &str) -> Option<String> {
        std::fs::read_to_string(dir.join(name)).ok()
    }

    #[test]
    fn migration_carries_the_credential_to_the_new_base() {
        let dir = fresh_dir("psmux_migrate_carries_key");
        seed_server(&dir, "__warm__");

        migrate_server_state_in(&dir, "__warm__", "work", 6001);

        assert_eq!(read(&dir, "work.key").as_deref(), Some("s3cret"));
        assert_eq!(read(&dir, "work.port").as_deref(), Some("6001"));
    }

    #[test]
    fn migration_leaves_no_files_at_the_old_base() {
        let dir = fresh_dir("psmux_migrate_clears_old");
        seed_server(&dir, "__warm__");

        migrate_server_state_in(&dir, "__warm__", "work", 6001);

        for ext in ["port", "key", "version"] {
            let old = dir.join(format!("__warm__.{}", ext));
            assert!(
                !old.exists(),
                "{} should not survive the move",
                old.display()
            );
        }
    }

    #[test]
    fn migration_moves_the_pipe_discovery_file() {
        let dir = fresh_dir("psmux_migrate_moves_pipe");
        seed_server(&dir, "__warm__");
        std::fs::write(dir.join("__warm__.pipe"), r"\.\pipe\psmux-warm").expect("seed pipe");

        migrate_server_state_in(&dir, "__warm__", "work", 6001);

        assert_eq!(
            read(&dir, "work.pipe").as_deref(),
            Some(r"\.\pipe\psmux-warm")
        );
        assert!(!dir.join("__warm__.pipe").exists());
    }

    #[test]
    fn a_concurrent_sweep_during_migration_cannot_strip_the_credential() {
        // Regression: the claim/rename paths used to unlink the old `.port`
        // before reading the old `.key`, so a sweep landing in that window
        // deleted the key and the new base never got one — leaving a session
        // with a valid `.port` and no credential.
        let dir = fresh_dir("psmux_migrate_survives_sweep");
        seed_server(&dir, "__warm__");
        // A live listener, so pass 1 sees the migrated server as alive and the
        // assertion is about pass 2 rather than about port liveness.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe listener");
        let port = listener.local_addr().expect("local_addr").port();

        migrate_server_state_in(&dir, "__warm__", "work", port);
        // Sweep with no grace at all — the harshest possible timing.
        cleanup_stale_state_in(&dir, Duration::ZERO);

        assert_eq!(
            read(&dir, "work.key").as_deref(),
            Some("s3cret"),
            "credential must survive a sweep immediately after migration"
        );
    }

    #[test]
    fn migration_to_the_same_base_is_a_no_op() {
        let dir = fresh_dir("psmux_migrate_same_base");
        seed_server(&dir, "work");

        migrate_server_state_in(&dir, "work", "work", 6001);

        assert_eq!(read(&dir, "work.key").as_deref(), Some("s3cret"));
        assert!(
            dir.join("work.port").exists(),
            "must not delete its own port"
        );
    }

    #[test]
    fn sweep_spares_an_unparented_sidecar_inside_the_grace_window() {
        let dir = fresh_dir("psmux_sweep_grace_spares_fresh");
        std::fs::write(dir.join("starting.key"), "s3cret").expect("seed");

        cleanup_stale_state_in(&dir, Duration::from_secs(60));

        assert!(
            dir.join("starting.key").exists(),
            "a sidecar written moments ago is a server mid-startup, not an orphan"
        );
    }
}

#[cfg(test)]
mod server_refusal_tests {
    use super::*;

    // The two strings the server actually writes, from
    // src/server/connection.rs:94 and :100.
    const AUTH_REQUIRED: &str = "ERROR: Authentication required";
    const INVALID_KEY: &str = "ERROR: Invalid session key";

    #[test]
    fn authentication_required_is_a_refusal() {
        assert!(is_server_refusal(AUTH_REQUIRED));
    }

    #[test]
    fn invalid_session_key_is_a_refusal() {
        assert!(is_server_refusal(INVALID_KEY));
    }

    #[test]
    fn a_refusal_is_recognised_with_its_trailing_newline() {
        // The server writes the string with a trailing \n; on the wire it may
        // also arrive \r\n.
        assert!(is_server_refusal(&format!("{}\n", AUTH_REQUIRED)));
        assert!(is_server_refusal(&format!("{}\r\n", INVALID_KEY)));
    }

    #[test]
    fn pane_content_beginning_with_error_is_not_a_refusal() {
        // The reason this matches whole payloads instead of an "ERROR:" prefix.
        // capture-pane and show-buffer return arbitrary pane content, and a
        // build log routinely starts with the word ERROR. Treating that as a
        // refusal would turn real output into a hard failure.
        assert!(!is_server_refusal(
            "ERROR: Authentication required by the API"
        ));
        assert!(!is_server_refusal("ERROR: could not compile psmux"));
        assert!(!is_server_refusal(
            "ERROR: Invalid session key\nnext line of pane output"
        ));
    }

    #[test]
    fn ordinary_output_is_not_a_refusal() {
        assert!(!is_server_refusal(""));
        assert!(!is_server_refusal("0: bash* (1 panes)"));
    }

    #[test]
    fn an_unresolvable_pane_target_is_an_error() {
        assert!(is_unresolved_target("can't find pane: %999"));
        assert!(is_unresolved_target("can't find pane: sess:0.%999\n"));
    }

    #[test]
    fn an_unresolvable_window_target_is_an_error() {
        assert!(is_unresolved_target("can't find window: sess:nosuchwindow"));
        assert!(is_unresolved_target("can't find window: @9\r\n"));
    }

    #[test]
    fn captured_pane_content_quoting_the_error_is_not_an_error() {
        // Same reason `is_server_refusal` matches whole payloads: capture-pane
        // returns arbitrary screen content, and a screen showing this message
        // among other lines is output, not a failed command.
        assert!(!is_unresolved_target(
            "can't find pane: %999\n$ echo done\ndone"
        ));
        assert!(!is_unresolved_target(
            "$ psmux kill-pane -t %999\ncan't find pane: %999"
        ));
    }

    // ── Contract between the writer (connection.rs) and this classifier ──
    //
    // These live in different files with no shared type, so the only thing
    // keeping them in agreement is the shared constant. Before it existed there
    // were two spellings in the tree — `"can't find pane: "` here and
    // `"can't find pane:"` (no trailing space) at two sites in main.rs.

    #[test]
    fn every_reply_the_server_writes_is_classified_as_an_error() {
        // Mirrors connection.rs's `writeln!("{}{}", prefix, raw)` exactly. If
        // the writer's format changes and this is not updated, the exit code
        // silently reverts to 0 — the failure the reply exists to prevent.
        for prefix in [
            crate::types::UNRESOLVED_PANE_PREFIX,
            crate::types::UNRESOLVED_WINDOW_PREFIX,
        ] {
            for raw in [
                "%999",
                "@9",
                "sess:nosuchwindow",
                "sess:0.%999",
                "=sess:0.1",
                "a name with spaces",
                "",
            ] {
                let wire = format!("{prefix}{raw}\n");
                assert!(
                    is_unresolved_target(&wire),
                    "server writes {wire:?} and the client must call it an error",
                );
            }
        }
    }

    #[test]
    fn the_two_prefixes_are_distinct_and_end_at_a_separator() {
        // The trailing space is load-bearing: without it `can't find pane` —
        // a truncated or unrelated line — would classify as a failed command.
        assert_ne!(
            crate::types::UNRESOLVED_PANE_PREFIX,
            crate::types::UNRESOLVED_WINDOW_PREFIX
        );
        for p in [
            crate::types::UNRESOLVED_PANE_PREFIX,
            crate::types::UNRESOLVED_WINDOW_PREFIX,
        ] {
            assert!(p.ends_with(": "), "{p:?} must end at a separator");
        }
    }

    #[test]
    fn ordinary_output_is_not_an_unresolvable_target() {
        assert!(!is_unresolved_target(""));
        assert!(!is_unresolved_target("0: bash* (1 panes)"));
        assert!(!is_unresolved_target("can't find pane"));
    }
}

#[cfg(test)]
mod one_shot_exchange_tests {
    use super::*;
    use std::io::Read;
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;

    /// Read timeout the client uses in these tests. Short enough to keep a
    /// stall test fast, long enough that a healthy loopback round trip on a
    /// loaded CI box never trips it.
    const TEST_READ_TIMEOUT: Duration = Duration::from_millis(400);

    /// A fake control server that answers exactly one connection.
    ///
    /// It reads the request **to EOF before replying**, which is the pin on the
    /// half-close: without `shutdown(Write)` that read never returns and every
    /// test here times out instead of getting its reply.
    ///
    /// Returns the port and a channel carrying the raw request bytes.
    fn spawn_fake_server(
        payload: &'static str,
        close_after_reply: bool,
    ) -> (u16, mpsc::Receiver<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake server");
        let port = listener.local_addr().expect("local_addr").port();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let _ = sock.read_to_end(&mut request);
            let _ = tx.send(request);
            let _ = sock.write_all(payload.as_bytes());
            let _ = sock.flush();
            if !close_after_reply {
                // Hold the socket open past the client's read timeout so the
                // client sees a stall rather than end-of-reply.
                std::thread::sleep(TEST_READ_TIMEOUT * 6);
            }
        });
        (port, rx)
    }

    fn client(port: u16) -> TcpStream {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let stream = TcpStream::connect_timeout(&addr, CONTROL_CONNECT_TIMEOUT)
            .expect("connect to fake server");
        stream
            .set_read_timeout(Some(TEST_READ_TIMEOUT))
            .expect("set read timeout");
        stream
    }

    #[test]
    fn a_complete_reply_returns_the_payload() {
        let (port, _rx) = spawn_fake_server("OK\n0: bash* (1 panes)\n", true);

        let got = exchange_one_shot(&mut client(port), "s3cret", None, "list-windows\n")
            .expect("a server that replies and closes must succeed");

        assert_eq!(got, "0: bash* (1 panes)\n");
    }

    #[test]
    fn the_request_reaches_the_server_and_ends_in_eof() {
        let (port, rx) = spawn_fake_server("OK\n", true);

        let _ = exchange_one_shot(&mut client(port), "s3cret", Some("work:1.2"), "kill-pane\n");

        let request = rx.recv().expect("server must reach EOF on the request");
        assert_eq!(
            String::from_utf8_lossy(&request),
            "AUTH s3cret\nTARGET work:1.2\nkill-pane\n"
        );
    }

    #[test]
    fn a_server_that_accepts_then_stalls_is_an_error() {
        // Reply framing only, then silence — the shape a wedged server has.
        let (port, _rx) = spawn_fake_server("OK\n", false);

        let err = exchange_one_shot(&mut client(port), "s3cret", None, "list-windows\n")
            .expect_err("a stall must not read as an empty successful reply");

        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn a_truncated_reply_is_an_error() {
        // The reply started and then stopped mid-record. Returning Ok here fed
        // half a record to machine consumers at rc 0.
        let (port, _rx) = spawn_fake_server("OK\n0: bash* (1 pa", false);

        let err = exchange_one_shot(&mut client(port), "s3cret", None, "list-windows\n")
            .expect_err("a truncated reply must not read as success");

        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(
            err.to_string().contains("incomplete reply"),
            "the error should say what happened, got: {}",
            err
        );
    }

    #[test]
    fn a_refusal_outranks_a_stall() {
        // A refusal is written before the OK ack and the connection may then
        // hang. Diagnosing that as a generic timeout would lose the actionable
        // half of the message, so classification runs first.
        let (port, _rx) = spawn_fake_server("ERROR: Invalid session key\n", false);

        let err = exchange_one_shot(&mut client(port), "wrong-key", None, "list-windows\n")
            .expect_err("a refusal is not command output");

        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(err.to_string(), "ERROR: Invalid session key");
    }
}
