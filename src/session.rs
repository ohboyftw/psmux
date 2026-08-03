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
    let home = match env::var("USERPROFILE").or_else(|_| env::var("HOME")) {
        Ok(h) => h,
        Err(_) => return "0".to_string(),
    };
    let psmux_dir = format!("{}\\.psmux", home);
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

/// Clean up any stale port files (where server is not actually running)
pub fn cleanup_stale_port_files() {
    let home = match env::var("USERPROFILE").or_else(|_| env::var("HOME")) {
        Ok(h) => h,
        Err(_) => return,
    };
    let psmux_dir = format!("{}\\.psmux", home);
    if let Ok(entries) = std::fs::read_dir(&psmux_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "port").unwrap_or(false) {
                if let Ok(port_str) = std::fs::read_to_string(&path) {
                    if let Ok(port) = port_str.trim().parse::<u16>() {
                        let addr = format!("127.0.0.1:{}", port);
                        if std::net::TcpStream::connect_timeout(
                            &addr.parse().unwrap(),
                            Duration::from_millis(50),
                        )
                        .is_err()
                        {
                            let _ = std::fs::remove_file(&path);
                            // Also remove the matching .key and .version files to
                            // prevent orphans from accumulating (issue #136).
                            let key_path = path.with_extension("key");
                            let _ = std::fs::remove_file(&key_path);
                            let ver_path = path.with_extension("version");
                            let _ = std::fs::remove_file(&ver_path);
                        }
                    } else {
                        let _ = std::fs::remove_file(&path);
                        let key_path = path.with_extension("key");
                        let _ = std::fs::remove_file(&key_path);
                    }
                }
            }
        }
    }
}

/// Kill any warm (standby) servers in the given namespace.
/// Called when the last non-warm session exits so orphan warm servers don't
/// linger indefinitely (#120, #138).  Skips if other non-warm sessions still
/// exist (warm servers may be needed for `new-session`).
pub fn kill_warm_servers(ns_prefix: Option<&str>) {
    let home = match env::var("USERPROFILE").or_else(|_| env::var("HOME")) {
        Ok(h) => h,
        Err(_) => return,
    };
    let psmux_dir = format!("{}\\.psmux", home);
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
    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .unwrap_or_default();
    let keypath = format!("{}\\.psmux\\{}.key", home, session);
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
    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .unwrap_or_default();
    let dir = format!("{}\\.psmux", home);
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

fn scan_servers_for_id(home: &str, target_id: &str) -> Option<(String, u16, String)> {
    // Bias toward the caller's own server when scripting from inside a psmux
    // pane: PSMUX_SESSION names the server that owns this process, so pane-ids
    // it mints are guaranteed to belong to it (unambiguous). Fall back to
    // reverse-mtime across all live servers otherwise.
    let own_session = env::var("PSMUX_SESSION")
        .ok()
        .filter(|s| !s.is_empty() && !is_warm_session(s));
    let dir = format!("{}\\.psmux", home);
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
        let path = format!("{}\\.psmux\\{}.port", home, name);
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
fn resolve_server_for_command(home: &str, line: &str) -> io::Result<(String, u16, String)> {
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
            if let Some(r) = scan_servers_for_id(home, &raw) {
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
                let path = format!("{}\\.psmux\\{}.port", home, name);
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
    let path = format!("{}\\.psmux\\{}.port", home, target);
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
    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .unwrap_or_default();
    let (_target, port, session_key) = resolve_server_for_command(&home, &line)?;
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

pub fn send_control_with_response(line: String) -> io::Result<String> {
    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .unwrap_or_default();
    let (_target, port, session_key) = resolve_server_for_command(&home, &line)?;
    let full_target = env::var("PSMUX_TARGET_FULL").ok();
    let addr = format!("127.0.0.1:{}", port);
    let mut stream = std::net::TcpStream::connect(&addr)?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(2000)));
    let _ = writeln!(stream, "AUTH {}", session_key);
    if let Some(ref ft) = full_target {
        let _ = writeln!(stream, "TARGET {}", ft);
    }
    let _ = write!(stream, "{}", line);
    let _ = stream.flush();
    let mut buf = Vec::new();
    let mut temp = [0u8; 4096];
    loop {
        match std::io::Read::read(&mut stream, &mut temp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&temp[..n]),
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                break
            }
            Err(_) => break,
        }
    }
    let result = String::from_utf8_lossy(&buf).to_string();
    // Strip the "OK\n" AUTH response prefix if present
    let result = if let Some(rest) = result.strip_prefix("OK\n") {
        rest.to_string()
    } else if let Some(rest) = result.strip_prefix("OK\r\n") {
        rest.to_string()
    } else {
        result
    };
    Ok(result)
}

/// Send a control message and wait for the full response with a custom timeout.
///
/// This is like `send_control_with_response` but allows specifying a longer
/// read timeout, which is needed for blocking commands like `wait-pane` that
/// may take many seconds (or minutes) to complete.
///
/// Pass `None` for `timeout` to block indefinitely (no read timeout).
pub fn send_control_with_response_timeout(
    line: String,
    timeout: Option<Duration>,
) -> io::Result<String> {
    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .unwrap_or_default();
    let (_target, port, session_key) = resolve_server_for_command(&home, &line)?;
    let full_target = env::var("PSMUX_TARGET_FULL").ok();
    let addr = format!("127.0.0.1:{}", port);
    let mut stream = std::net::TcpStream::connect(&addr)?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(timeout);
    let _ = writeln!(stream, "AUTH {}", session_key);
    if let Some(ref ft) = full_target {
        let _ = writeln!(stream, "TARGET {}", ft);
    }
    let _ = write!(stream, "{}", line);
    let _ = stream.flush();
    let mut buf = Vec::new();
    let mut temp = [0u8; 4096];
    loop {
        match std::io::Read::read(&mut stream, &mut temp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&temp[..n]),
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                break
            }
            Err(_) => break,
        }
    }
    let result = String::from_utf8_lossy(&buf).to_string();
    // Strip the "OK\n" AUTH response prefix if present
    let result = if let Some(rest) = result.strip_prefix("OK\n") {
        rest.to_string()
    } else if let Some(rest) = result.strip_prefix("OK\r\n") {
        rest.to_string()
    } else {
        result
    };
    Ok(result)
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
    let home = env::var("USERPROFILE").or_else(|_| env::var("HOME")).ok()?;
    let dir = format!("{}\\.psmux", home);
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
        let home = env::var("USERPROFILE").or_else(|_| env::var("HOME")).ok()?;
        let p = format!("{}\\.psmux\\{}.port", home, name);
        if std::path::Path::new(&p).exists() {
            return Some(name);
        }
    }
    let home = env::var("USERPROFILE").or_else(|_| env::var("HOME")).ok()?;
    let candidates = [
        format!("{}\\.psmuxrc", home),
        format!("{}\\.psmux\\pmuxrc", home),
    ];
    for cfg in candidates.iter() {
        if let Ok(text) = std::fs::read_to_string(cfg) {
            let line = text.lines().find(|l| !l.trim().is_empty())?;
            let name = if let Some(rest) = line.strip_prefix("default-session ") {
                rest.trim().to_string()
            } else {
                line.trim().to_string()
            };
            let p = format!("{}\\.psmux\\{}.port", home, name);
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
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    let dir = format!("{}\\.psmux", home);
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
    let home = match env::var("USERPROFILE").or_else(|_| env::var("HOME")) {
        Ok(h) => h,
        Err(_) => return vec![],
    };
    let psmux_dir = format!("{}\\.psmux", home);
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
