//! The `~/.mycel/server` discovery file.
//!
//! Format, as every existing reader expects: one JSON object with a string
//! `host` and a numeric `port`, optionally `tailscale_ip`. psmux adds `pid`,
//! which readers ignore (serde skips unknown keys, Python uses `.get`).
//!
//! Every function takes the file path explicitly so tests never touch the
//! real file. Writes go to a temp file and are renamed into place, and a
//! bus removes the file on exit only while it still names that bus.

use std::fs;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Port a bus listens on when nothing else is configured.
pub const DEFAULT_PORT: u16 = 4321;

/// Contents of the discovery file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryInfo {
    /// Host to connect to. A psmux bus always writes `127.0.0.1`.
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// Tailscale address written by the C++ server; psmux never sets it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailscale_ip: Option<String>,
    /// Process id of the psmux bus that wrote the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

impl DiscoveryInfo {
    /// The address of a psmux bus on `port` in this process.
    pub fn for_local_bus(port: u16) -> Self {
        DiscoveryInfo {
            host: "127.0.0.1".into(),
            port,
            tailscale_ip: None,
            pid: Some(std::process::id()),
        }
    }

    /// Resolve `host:port`, IPv4 first. On Windows `localhost` resolves to
    /// `::1` first, and a refused loopback connect takes about 2 s.
    pub fn socket_addr(&self) -> Option<SocketAddr> {
        let addrs: Vec<SocketAddr> = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .ok()?
            .collect();
        addrs
            .iter()
            .find(|a| a.is_ipv4())
            .or(addrs.first())
            .copied()
    }
}

/// `%USERPROFILE%\.mycel\server`, falling back to `$HOME/.mycel/server`,
/// the same order the C++ server and the Rust client use.
pub fn default_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join(".mycel").join("server"))
}

/// Read and parse the discovery file. `None` if missing or unparseable.
pub fn read(path: &Path) -> Option<DiscoveryInfo> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write the discovery file atomically: a sibling temp file renamed over
/// the target. Creates the parent directory if needed.
pub fn write(path: &Path, info: &DiscoveryInfo) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut text = serde_json::to_string(info).map_err(io::Error::other)?;
    text.push('\n');
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// Remove the discovery file if it still names the bus `ours` wrote.
/// Another host may have replaced it since; that file is left alone.
pub fn remove_if_ours(path: &Path, ours: &DiscoveryInfo) {
    if read(path).as_ref() == Some(ours) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
#[path = "../../tests-rs/test_bus_discovery.rs"]
mod tests;
