//! Publishing psmux lifecycle events on the mycel bus.
//!
//! A [`MycelBus`] owns a background thread with a blocking in-tree client
//! ([`crate::bus::client`]). On each publish it reuses its connection, or
//! looks for a live bus (discovery file first, then `127.0.0.1:4321`) and,
//! if none answers, starts `psmux bus` once and waits for it. Set
//! `PSMUX_BUS=0` to never start a bus (an existing one is still used).
//!
//! The bus is an optional observability channel: when it cannot be
//! reached, events are dropped and the next attempt waits out a backoff,
//! so a missing bus never costs more than one connect timeout per backoff.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::bus::client::Client;
use crate::bus::host::{spawn_bus_process, BusLocation};

/// Events waiting for the publisher thread; more are dropped.
const QUEUE_CAPACITY: usize = 1024;
/// Connect and Hello timeout for an address that should be live.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(1500);
/// Connect timeout while polling a bus that is starting.
const POLL_TIMEOUT: Duration = Duration::from_millis(300);
/// How long to wait for a freshly started bus to answer.
const START_WAIT: Duration = Duration::from_secs(8);
/// Least time between two attempts to start a bus.
const RESPAWN_INTERVAL: Duration = Duration::from_secs(30);
/// First and largest wait after a failed connect.
const BACKOFF_MIN: Duration = Duration::from_secs(2);
const BACKOFF_MAX: Duration = Duration::from_secs(60);

/// A message to publish on the mycel bus.
struct PublishMsg {
    topic: String,
    payload: Vec<u8>,
}

/// Canonical topic names published by psmux.
///
/// External subscribers (canopy, orchestrators, observability tools) depend
/// on these exact strings. Changing any topic here is a breaking change.
pub mod topics {
    pub const PANE_CREATED: &str = "psmux/pane/created";
    pub const PANE_READY: &str = "psmux/pane/ready";
    pub const PANE_EXITED: &str = "psmux/pane/exited";
    /// A pane's reported (OSC 7501) worst-of agent state changed.
    pub const PANE_STATUS: &str = "psmux/pane/status";
    pub const EXEC_COMPLETED: &str = "psmux/exec/completed";
    pub const SESSION_CREATED: &str = "psmux/session/created";
    pub const SESSION_RENAMED: &str = "psmux/session/renamed";
    pub const SESSION_KILLED: &str = "psmux/session/killed";
}

/// Handle to the mycel background publisher thread. Send-only.
pub struct MycelBus {
    tx: mpsc::SyncSender<PublishMsg>,
    client_id: String,
}

impl MycelBus {
    /// Start the publisher thread. Nothing connects until the first
    /// publish. `client_id` is reported as the `publisher` of every event
    /// (e.g. `"psmux@hostname"`).
    pub fn new(client_id: &str) -> Self {
        Self::with_link(Link::new(
            client_id,
            BusLocation::from_env(),
            autostart_exe(),
        ))
    }

    fn with_link(mut link: Link) -> Self {
        let (tx, rx) = mpsc::sync_channel::<PublishMsg>(QUEUE_CAPACITY);
        let client_id = link.client_id.clone();
        let spawned = thread::Builder::new()
            .name("mycel-bus".into())
            .spawn(move || {
                while let Ok(msg) = rx.recv() {
                    link.publish(&msg);
                }
            });
        if spawned.is_err() {
            crate::debug_log::server_log("mycel", "failed to spawn publisher thread");
        }
        Self { tx, client_id }
    }

    /// The client_id this bus was initialised with (e.g. `"psmux@hostname"`).
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Publish a JSON payload to a topic. Never blocks: when the queue is
    /// full or the thread is gone, the event is dropped.
    pub fn publish(&self, topic: &str, payload: &impl serde::Serialize) {
        if let Ok(bytes) = serde_json::to_vec(payload) {
            let _ = self.tx.try_send(PublishMsg {
                topic: topic.to_string(),
                payload: bytes,
            });
        }
    }
}

/// The publisher thread's connection state.
struct Link {
    client_id: String,
    loc: BusLocation,
    /// The psmux executable to start a bus with; `None` never starts one.
    autostart: Option<PathBuf>,
    client: Option<Client>,
    retry_at: Option<Instant>,
    backoff: Duration,
    last_spawn: Option<Instant>,
}

impl Link {
    fn new(client_id: &str, loc: BusLocation, autostart: Option<PathBuf>) -> Self {
        Link {
            client_id: client_id.into(),
            loc,
            autostart,
            client: None,
            retry_at: None,
            backoff: BACKOFF_MIN,
            last_spawn: None,
        }
    }

    fn publish(&mut self, msg: &PublishMsg) {
        // A server that restarted closed our socket; without this check the
        // next write would vanish into the kernel buffer.
        if self.client.as_ref().is_some_and(Client::peer_closed) {
            self.client = None;
        }
        if self.client.is_none() && !self.connect() {
            return;
        }
        if let Some(c) = self.client.as_mut() {
            if c.publish(&msg.topic, &msg.payload, HashMap::new()).is_err() {
                self.client = None;
            }
        }
    }

    fn connect(&mut self) -> bool {
        if self.retry_at.is_some_and(|t| Instant::now() < t) {
            return false;
        }
        let client = self.connect_existing().or_else(|| self.start_and_connect());
        match client {
            Some(c) => {
                self.client = Some(c);
                self.retry_at = None;
                self.backoff = BACKOFF_MIN;
                true
            }
            None => {
                self.retry_at = Some(Instant::now() + self.backoff);
                self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
                false
            }
        }
    }

    /// Connect to the first candidate address that answers Hello. The
    /// discovery file is re-read every time, so a bus that moved is found.
    fn connect_existing(&self) -> Option<Client> {
        self.loc
            .candidates()
            .into_iter()
            .find_map(|a| self.try_connect(a, CONNECT_TIMEOUT))
    }

    fn try_connect(&self, addr: SocketAddr, timeout: Duration) -> Option<Client> {
        Client::connect(addr, &self.client_id, timeout).ok()
    }

    /// Start `psmux bus` (at most once per [`RESPAWN_INTERVAL`]) and wait
    /// for it, or for whichever bus won a start race, to answer.
    fn start_and_connect(&mut self) -> Option<Client> {
        let exe = self.autostart.as_ref()?;
        if self
            .last_spawn
            .is_some_and(|t| t.elapsed() < RESPAWN_INTERVAL)
        {
            return None;
        }
        self.last_spawn = Some(Instant::now());
        if let Err(e) = spawn_bus_process(exe, &self.loc) {
            crate::debug_log::server_log("mycel", &format!("cannot start psmux bus: {e}"));
            return None;
        }
        let deadline = Instant::now() + START_WAIT;
        while Instant::now() < deadline {
            let found = self
                .loc
                .candidates()
                .into_iter()
                .find_map(|a| self.try_connect(a, POLL_TIMEOUT));
            if found.is_some() {
                return found;
            }
            thread::sleep(Duration::from_millis(100));
        }
        None
    }
}

/// The executable to start a bus with: this process's own image, but only
/// when it is psmux (not a test harness, which would read `bus` as a test
/// filter), and only when `PSMUX_BUS` is not `0`.
fn autostart_exe() -> Option<PathBuf> {
    if std::env::var("PSMUX_BUS").is_ok_and(|v| v.trim() == "0") {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let stem = exe.file_stem()?.to_str()?.to_ascii_lowercase();
    matches!(stem.as_str(), "psmux" | "pmux" | "tmux").then_some(exe)
}

/// Global mycel bus instance.
///
/// Initialized once by `init_mycel_bus()`, accessed via `mycel_bus()`.
static MYCEL_BUS: std::sync::OnceLock<MycelBus> = std::sync::OnceLock::new();

/// Initialize the global mycel bus. Call once at server startup.
/// No-op if already initialized.
pub fn init_mycel_bus(client_id: &str) {
    MYCEL_BUS.get_or_init(|| MycelBus::new(client_id));
}

/// Get a reference to the global mycel bus, if initialized.
pub fn mycel_bus() -> Option<&'static MycelBus> {
    MYCEL_BUS.get()
}

/// Return the client_id the global bus was initialised with.
/// Returns `None` if the bus has not been initialised.
pub fn mycel_client_id() -> Option<&'static str> {
    MYCEL_BUS.get().map(|b| b.client_id())
}

/// Convenience: publish a pane lifecycle event if the bus is initialised.
pub fn publish_pane_event(topic: &str, payload: &impl serde::Serialize) {
    if let Some(bus) = mycel_bus() {
        bus.publish(topic, payload);
    }
}

#[cfg(test)]
#[path = "../tests-rs/test_mycel_publisher.rs"]
mod tests;
