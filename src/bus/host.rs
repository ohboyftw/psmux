//! Finding a live bus, and the `psmux bus` process that hosts one.
//!
//! A bus is "live" when it answers Hello. The discovery file is tried
//! first, then `127.0.0.1:<port>`. A discovery file whose address does not
//! answer is stale and is overwritten by the next host.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::client::{probe, Client, ClientError, PROBE_CLIENT_ID};
use super::discovery::{self, DEFAULT_PORT};
use super::server::{start, BusConfig};
use crate::detached::{spawn_detached, Console};

/// How long a liveness probe waits for the Hello Ack.
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// Env var naming the bus port (default 4321).
pub const PORT_ENV: &str = "PSMUX_BUS_PORT";
/// Env var naming the discovery file (default `~/.mycel/server`).
pub const DISCOVERY_ENV: &str = "PSMUX_BUS_DISCOVERY";

/// Where a bus is looked for and hosted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusLocation {
    /// Port to host on, and to probe when the discovery file is absent.
    pub port: u16,
    /// Discovery file, if one can be located.
    pub discovery_path: Option<PathBuf>,
}

impl BusLocation {
    /// The location from `PSMUX_BUS_PORT` and `PSMUX_BUS_DISCOVERY`,
    /// defaulting to 4321 and `~/.mycel/server`.
    pub fn from_env() -> Self {
        let port = std::env::var(PORT_ENV)
            .ok()
            .and_then(|p| p.trim().parse().ok())
            .unwrap_or(DEFAULT_PORT);
        let discovery_path = std::env::var_os(DISCOVERY_ENV)
            .map(PathBuf::from)
            .or_else(discovery::default_path);
        BusLocation {
            port,
            discovery_path,
        }
    }

    /// The loopback address a psmux bus at this location listens on.
    pub fn local_addr(&self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, self.port))
    }

    /// Candidate addresses, discovery file first, without duplicates.
    pub fn candidates(&self) -> Vec<SocketAddr> {
        let mut out = Vec::new();
        let from_file = self
            .discovery_path
            .as_deref()
            .and_then(discovery::read)
            .and_then(|info| info.socket_addr());
        out.extend(from_file);
        if !out.contains(&self.local_addr()) {
            out.push(self.local_addr());
        }
        out
    }
}

/// The first candidate address that answers Hello, if any.
pub fn find_live_bus(loc: &BusLocation, timeout: Duration) -> Option<SocketAddr> {
    loc.candidates().into_iter().find(|a| probe(*a, timeout))
}

/// The pid of the psmux bus the discovery file at `path` names, while that
/// bus still holds its port and `is_psmux_pid` says the pid is a psmux
/// process. Only a psmux bus writes `pid`; a standalone mycel writes none.
pub fn shared_bus_pid(path: &Path, is_psmux_pid: impl Fn(u32) -> bool) -> Option<u32> {
    let info = discovery::read(path)?;
    let pid = info.pid?;
    let held = std::net::TcpListener::bind(info.socket_addr()?).is_err();
    (held && is_psmux_pid(pid)).then_some(pid)
}

/// Whether `pid` is a running psmux, pmux or tmux image.
pub fn is_psmux_pid(pid: u32) -> bool {
    crate::warm_reap::running_image_stem(pid).is_some_and(|name| {
        matches!(
            name.to_ascii_lowercase().as_str(),
            "psmux" | "pmux" | "tmux"
        )
    })
}

/// Parse `psmux bus [--port N] [--discovery PATH]` arguments (the words
/// after `bus`).
pub fn parse_args(args: &[&str], mut loc: BusLocation) -> Result<BusLocation, String> {
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match *arg {
            "--port" => {
                let v = it.next().ok_or("--port needs a value")?;
                loc.port = v.parse().map_err(|_| format!("invalid port '{v}'"))?;
            }
            "--discovery" => {
                let v = it.next().ok_or("--discovery needs a path")?;
                loc.discovery_path = Some(PathBuf::from(v));
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(loc)
}

/// `psmux bus [--port N] [--discovery PATH]` hosts a bus;
/// `psmux bus stop [...]` asks the live one to shut down.
pub fn run_bus_command(words: &[&str]) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    let (stop, rest) = match words.split_first() {
        Some((&"stop", rest)) => (true, rest),
        _ => (false, words),
    };
    let loc = parse_args(rest, BusLocation::from_env())
        .map_err(|e| Error::new(ErrorKind::InvalidInput, e))?;
    if !stop {
        return run_bus_process(&loc);
    }
    match stop_live_bus(&loc) {
        Ok(true) => println!("mycel bus stopped"),
        Ok(false) => println!("no mycel bus is running"),
        Err(e) => return Err(Error::other(e.to_string())),
    }
    Ok(())
}

/// Run `psmux bus`: defer to a live bus if one answers, otherwise host on
/// `127.0.0.1:<port>` until a Shutdown request. Losing a start race (the
/// port is already bound) is not an error.
pub fn run_bus_process(loc: &BusLocation) -> std::io::Result<()> {
    if let Some(addr) = find_live_bus(loc, PROBE_TIMEOUT) {
        log(&format!("a mycel bus is already live at {addr}"));
        return Ok(());
    }
    let cfg = BusConfig::new(loc.local_addr(), loc.discovery_path.clone());
    match start(cfg) {
        Ok(handle) => {
            handle.join();
            Ok(())
        }
        Err(e) if is_port_taken(&e) => {
            log(&format!("port {} is taken; not hosting", loc.port));
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Run `psmux bus stop`: ask the live bus, if any, to shut down. Returns
/// whether a bus answered. The next psmux event starts a new one unless
/// `PSMUX_BUS=0` (publishing off) was set before the session servers
/// started.
pub fn stop_live_bus(loc: &BusLocation) -> Result<bool, ClientError> {
    let Some(addr) = find_live_bus(loc, PROBE_TIMEOUT) else {
        return Ok(false);
    };
    let mut client = Client::connect(addr, PROBE_CLIENT_ID, PROBE_TIMEOUT)?;
    client.set_read_timeout(Some(PROBE_TIMEOUT))?;
    client.shutdown("psmux bus stop")?;
    Ok(true)
}

/// Start `exe bus --port P [--discovery PATH]` as a detached background
/// process with no console window and no inherited handles, working in
/// the user's home directory so it never pins the directory psmux was
/// started from.
pub fn spawn_bus_process(exe: &Path, loc: &BusLocation) -> std::io::Result<()> {
    let mut args = vec!["bus".to_owned(), "--port".to_owned(), loc.port.to_string()];
    if let Some(path) = &loc.discovery_path {
        args.push("--discovery".into());
        args.push(path.to_string_lossy().into_owned());
    }
    let cwd = bus_cwd(exe);
    spawn_detached(exe, &args, Console::None, cwd.as_deref())
}

/// `%USERPROFILE%` (or `$HOME`), else the executable's directory, which
/// the running image pins anyway.
fn bus_cwd(exe: &Path) -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|d| d.is_dir())
        .or_else(|| exe.parent().map(Path::to_path_buf))
}

/// The bus runs without a console or stdio, so messages go to the server
/// log (`PSMUX_DEBUG`) instead of stderr.
fn log(msg: &str) {
    crate::debug_log::server_log("bus", msg);
}

/// `AddrInUse`, or on Windows `PermissionDenied` when the holder used
/// `SO_EXCLUSIVEADDRUSE`.
fn is_port_taken(e: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    matches!(e.kind(), ErrorKind::AddrInUse | ErrorKind::PermissionDenied)
}

#[cfg(test)]
#[path = "../../tests-rs/test_bus_host.rs"]
mod tests;
