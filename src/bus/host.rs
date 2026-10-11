//! Finding a live bus, and the `psmux bus` process that hosts one.
//!
//! A bus is "live" when it answers Hello. The discovery file is tried
//! first, then `127.0.0.1:<port>`. A discovery file whose address does not
//! answer is stale and is overwritten by the next host.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use super::client::probe;
use super::discovery::{self, DEFAULT_PORT};
use super::server::{start, BusConfig};

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

/// Run `psmux bus`: defer to a live bus if one answers, otherwise host on
/// `127.0.0.1:<port>` until a Shutdown request. Losing a start race (the
/// port is already bound) is not an error.
pub fn run_bus_process(loc: &BusLocation) -> std::io::Result<()> {
    if let Some(addr) = find_live_bus(loc, PROBE_TIMEOUT) {
        eprintln!("psmux bus: a mycel bus is already live at {addr}");
        return Ok(());
    }
    let cfg = BusConfig::new(loc.local_addr(), loc.discovery_path.clone());
    match start(cfg) {
        Ok(handle) => {
            handle.join();
            Ok(())
        }
        Err(e) if is_port_taken(&e) => {
            eprintln!("psmux bus: port {} is taken; not hosting", loc.port);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Start `exe bus --port P [--discovery PATH]` as a detached background
/// process with no console window. On Windows it first tries to leave the
/// caller's job, so the bus outlives the session server that started it
/// (as `spawn_server_hidden` does); a job that forbids breakaway refuses
/// that, and the bus then starts inside the job.
pub fn spawn_bus_process(exe: &std::path::Path, loc: &BusLocation) -> std::io::Result<()> {
    let mut args = vec!["bus".to_owned(), "--port".to_owned(), loc.port.to_string()];
    if let Some(path) = &loc.discovery_path {
        args.push("--discovery".into());
        args.push(path.to_string_lossy().into_owned());
    }
    #[cfg(windows)]
    {
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let base = CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP;
        spawn_with_flags(exe, &args, base | CREATE_BREAKAWAY_FROM_JOB)
            .or_else(|_| spawn_with_flags(exe, &args, base))
    }
    #[cfg(not(windows))]
    {
        bus_command(exe, &args).spawn().map(drop)
    }
}

#[cfg(windows)]
fn spawn_with_flags(exe: &std::path::Path, args: &[String], flags: u32) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    bus_command(exe, args)
        .creation_flags(flags)
        .spawn()
        .map(drop)
}

fn bus_command(exe: &std::path::Path, args: &[String]) -> std::process::Command {
    use std::process::Stdio;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd
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
