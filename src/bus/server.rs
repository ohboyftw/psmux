//! The bus process: a TCP listener, one reader and one writer thread per
//! connection, and a heartbeat thread, all sharing one [`State`].
//!
//! Started by `psmux bus`, or in-process by tests on an ephemeral port.
//! The listener is a plain `std` bind, which on Windows does not set
//! `SO_REUSEADDR`, so a second host cannot bind the same port.

use std::collections::HashMap;
use std::io::{self, Write};
use std::net::{Shutdown as NetShutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::codec::{read_frame, FrameError};
use super::discovery::{self, DiscoveryInfo};
use super::proto::Envelope;
use super::state::{Action, Limits, Pop, State};
use super::subs::ConnId;

/// Control frames a connection may have waiting before its requests stop
/// being read.
const MAX_CONTROL_BACKLOG: usize = 4096;
/// How long a Shutdown waits for its Ack to be written.
const SHUTDOWN_FLUSH: Duration = Duration::from_secs(3);

/// How to run a bus.
#[derive(Debug, Clone)]
pub struct BusConfig {
    /// Address to listen on. Port 0 picks a free port.
    pub bind: SocketAddr,
    /// Discovery file to write after bind and remove on stop. `None`
    /// writes nothing (tests).
    pub discovery_path: Option<PathBuf>,
    /// Time a new connection has to send Hello.
    pub hello_timeout: Duration,
    /// Interval between heartbeat rounds.
    pub heartbeat_interval: Duration,
    /// Queue, history and connection limits.
    pub limits: Limits,
}

impl BusConfig {
    /// Defaults for a bus on `bind` that writes `discovery_path`.
    pub fn new(bind: SocketAddr, discovery_path: Option<PathBuf>) -> Self {
        BusConfig {
            bind,
            discovery_path,
            hello_timeout: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(30),
            limits: Limits::default(),
        }
    }
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    stop: AtomicBool,
    next_conn: AtomicU64,
    streams: Mutex<HashMap<ConnId, TcpStream>>,
    local_addr: SocketAddr,
    discovery: Option<(PathBuf, DiscoveryInfo)>,
    hello_timeout: Duration,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn stopping(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Stop everything: refuse new work, close every socket, wake the
    /// accept loop, and remove the discovery file if it is still ours.
    fn stop_bus(&self) {
        if self.stop.swap(true, Ordering::SeqCst) {
            return;
        }
        self.lock().stopping = true;
        self.wake.notify_all();
        let streams = self.streams.lock().unwrap_or_else(|p| p.into_inner());
        for s in streams.values() {
            let _ = s.shutdown(NetShutdown::Both);
        }
        drop(streams);
        let _ = TcpStream::connect_timeout(&self.local_addr, Duration::from_millis(500));
        if let Some((path, info)) = &self.discovery {
            discovery::remove_if_ours(path, info);
        }
    }
}

/// A running bus.
pub struct BusHandle {
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
    heartbeat: Option<JoinHandle<()>>,
}

impl BusHandle {
    /// The address the bus listens on.
    pub fn local_addr(&self) -> SocketAddr {
        self.shared.local_addr
    }

    /// Stop the bus, as a Shutdown request would.
    pub fn stop(&self) {
        self.shared.stop_bus();
    }

    /// Wait until the bus has stopped (after [`stop`](Self::stop) or a
    /// Shutdown request).
    pub fn join(mut self) {
        for h in [self.accept.take(), self.heartbeat.take()]
            .into_iter()
            .flatten()
        {
            let _ = h.join();
        }
    }
}

/// Current wall-clock time in Unix ms.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Bind, write the discovery file, announce `$mycel/server/started`, and
/// start serving. Fails if the port is taken.
pub fn start(cfg: BusConfig) -> io::Result<BusHandle> {
    let listener = TcpListener::bind(cfg.bind)?;
    let local_addr = listener.local_addr()?;
    let discovery = write_discovery(cfg.discovery_path, local_addr.port())?;
    let mut state = State::new(cfg.limits);
    state.server_started(local_addr.port(), &local_addr.ip().to_string(), now_ms());
    let shared = Arc::new(Shared {
        state: Mutex::new(state),
        wake: Condvar::new(),
        stop: AtomicBool::new(false),
        next_conn: AtomicU64::new(1),
        streams: Mutex::new(HashMap::new()),
        local_addr,
        discovery,
        hello_timeout: cfg.hello_timeout,
    });
    let accept = spawn_named("mycel-accept", {
        let shared = shared.clone();
        move || accept_loop(&shared, &listener)
    })?;
    let heartbeat = spawn_named("mycel-heartbeat", {
        let shared = shared.clone();
        move || heartbeat_loop(&shared, cfg.heartbeat_interval)
    })?;
    Ok(BusHandle {
        shared,
        accept: Some(accept),
        heartbeat: Some(heartbeat),
    })
}

/// Write the discovery file for a bus on `port`, if a path is given.
fn write_discovery(
    path: Option<PathBuf>,
    port: u16,
) -> io::Result<Option<(PathBuf, DiscoveryInfo)>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let info = DiscoveryInfo::for_local_bus(port);
    discovery::write(&path, &info)?;
    Ok(Some((path, info)))
}

fn spawn_named(name: &str, f: impl FnOnce() + Send + 'static) -> io::Result<JoinHandle<()>> {
    thread::Builder::new().name(name.into()).spawn(f)
}

fn accept_loop(shared: &Arc<Shared>, listener: &TcpListener) {
    for stream in listener.incoming() {
        if shared.stopping() {
            return;
        }
        let Ok(stream) = stream else {
            thread::sleep(Duration::from_millis(50));
            continue;
        };
        let conn = shared.next_conn.fetch_add(1, Ordering::SeqCst);
        let shared = shared.clone();
        let _ = spawn_named("mycel-conn", move || serve_conn(&shared, stream, conn));
    }
}

fn heartbeat_loop(shared: &Shared, interval: Duration) {
    let mut state = shared.lock();
    loop {
        let (guard, timeout) = shared
            .wake
            .wait_timeout_while(state, interval, |_| !shared.stopping())
            .unwrap_or_else(|p| p.into_inner());
        state = guard;
        if shared.stopping() {
            return;
        }
        if timeout.timed_out() {
            state.heartbeat(now_ms());
            shared.wake.notify_all();
        }
    }
}

fn serve_conn(shared: &Arc<Shared>, stream: TcpStream, conn: ConnId) {
    let registered = match stream.try_clone() {
        Ok(clone) => {
            shared
                .streams
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(conn, clone);
            read_requests(shared, &stream, conn)
        }
        Err(_) => false,
    };
    shared
        .streams
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&conn);
    if registered && shared.lock().disconnect(conn, now_ms()) {
        shared.wake.notify_all();
    }
    let _ = stream.shutdown(NetShutdown::Both);
}

/// Read and handle frames until the peer closes, the stream breaks, or
/// the bus stops. Every frame that arrived before EOF is handled first.
/// Returns whether the connection ever completed Hello.
fn read_requests(shared: &Arc<Shared>, stream: &TcpStream, conn: ConnId) -> bool {
    let mut registered = false;
    let _ = stream.set_read_timeout(Some(shared.hello_timeout));
    let mut reader = stream;
    while !shared.stopping() {
        let env = match read_frame(&mut reader) {
            Ok(Some(env)) => env,
            Err(FrameError::Decode(_)) => continue,
            Ok(None) | Err(_) => break,
        };
        match handle_frame(shared, conn, env) {
            Some(Action::Registered) => {
                registered = true;
                let _ = stream.set_read_timeout(None);
                if start_writer(shared, stream, conn).is_err() {
                    break;
                }
            }
            Some(Action::Reject(bytes)) => {
                let _ = (&*stream).write_all(&bytes);
                break;
            }
            Some(Action::Shutdown) => {
                finish_shutdown(shared);
                break;
            }
            Some(Action::Continue) | None => {}
        }
    }
    registered
}

fn handle_frame(shared: &Shared, conn: ConnId, env: Envelope) -> Option<Action> {
    let msg = env.msg?;
    let mut state = shared.lock();
    // Back-pressure on requests: a client that keeps asking but never
    // reads its replies stops being read until its writer catches up.
    while state.queued_control(conn) >= MAX_CONTROL_BACKLOG && !shared.stopping() {
        state = shared.wake.wait(state).unwrap_or_else(|p| p.into_inner());
    }
    let action = state.on_message(conn, msg, now_ms());
    drop(state);
    shared.wake.notify_all();
    Some(action)
}

/// Give the writer time to send the Shutdown Ack (it stops the bus
/// itself once the Ack is written), then stop the bus regardless, so a
/// client that never reads cannot keep it alive.
fn finish_shutdown(shared: &Shared) {
    let state = shared.lock();
    let _ = shared
        .wake
        .wait_timeout_while(state, SHUTDOWN_FLUSH, |_| !shared.stopping());
    shared.stop_bus();
}

fn start_writer(shared: &Arc<Shared>, stream: &TcpStream, conn: ConnId) -> io::Result<()> {
    let out = stream.try_clone()?;
    let shared = shared.clone();
    spawn_named("mycel-writer", move || write_frames(&shared, out, conn))?;
    Ok(())
}

/// Write queued frames in order until the connection goes away. A failed
/// write closes the socket so the reader runs the one disconnect path.
fn write_frames(shared: &Shared, mut out: TcpStream, conn: ConnId) {
    loop {
        let Some(frame) = next_frame(shared, conn) else {
            return;
        };
        if out.write_all(&frame.bytes).is_err() {
            let _ = out.shutdown(NetShutdown::Both);
            return;
        }
        shared.wake.notify_all();
        if frame.stop_bus {
            let _ = out.flush();
            shared.stop_bus();
            return;
        }
    }
}

fn next_frame(shared: &Shared, conn: ConnId) -> Option<super::state::Outgoing> {
    let mut state = shared.lock();
    loop {
        if shared.stopping() {
            return None;
        }
        match state.pop(conn) {
            Pop::Frame(f) => return Some(f),
            Pop::Gone => return None,
            Pop::Empty => {
                state = shared.wake.wait(state).unwrap_or_else(|p| p.into_inner());
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests-rs/test_bus_server.rs"]
mod tests;
