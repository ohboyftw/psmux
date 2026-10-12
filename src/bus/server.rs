//! The bus process: a TCP listener, one reader and one writer thread per
//! connection, and a heartbeat thread, all sharing one [`State`].
//!
//! Started by `psmux bus`, or in-process by tests on an ephemeral port.
//! The listener is a plain `std` bind without `SO_REUSEADDR`, so a second
//! plain bind of the port fails. On Windows a later socket that sets
//! `SO_REUSEADDR` (the C++ server's asio listener does) can still bind it
//! alongside, and the two servers then split clients between them.

use std::collections::HashMap;
use std::io::{self, Write};
use std::net::{Shutdown as NetShutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    /// Time a new connection has, from accept, to complete Hello.
    pub hello_timeout: Duration,
    /// Interval between heartbeat rounds.
    pub heartbeat_interval: Duration,
    /// Queue, history and connection limits.
    pub limits: Limits,
    /// Control frames a connection may have waiting before its requests
    /// stop being read.
    pub control_backlog: usize,
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
            control_backlog: MAX_CONTROL_BACKLOG,
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
    control_backlog: usize,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn stopping(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Stop everything: refuse new work, close every socket, remove the
    /// discovery file if it is still ours, and wake the accept loop.
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
        // Before the wake: once the accept loop returns, `join` can return
        // and the `psmux bus` process exit with the file still in place.
        if let Some((path, info)) = &self.discovery {
            discovery::remove_if_ours(path, info);
        }
        let _ = TcpStream::connect_timeout(&self.local_addr, Duration::from_millis(500));
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
/// start serving. Fails if the port is taken. A discovery file that cannot
/// be written is logged and skipped: clients also probe the port.
pub fn start(cfg: BusConfig) -> io::Result<BusHandle> {
    let listener = TcpListener::bind(cfg.bind)?;
    let local_addr = listener.local_addr()?;
    let discovery = write_discovery(cfg.discovery_path.clone(), local_addr.port());
    let shared = new_shared(&cfg, local_addr, discovery);
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

fn new_shared(
    cfg: &BusConfig,
    local_addr: SocketAddr,
    discovery: Option<(PathBuf, DiscoveryInfo)>,
) -> Arc<Shared> {
    let mut state = State::new(cfg.limits);
    state.server_started(local_addr.port(), &local_addr.ip().to_string(), now_ms());
    Arc::new(Shared {
        state: Mutex::new(state),
        wake: Condvar::new(),
        stop: AtomicBool::new(false),
        next_conn: AtomicU64::new(1),
        streams: Mutex::new(HashMap::new()),
        local_addr,
        discovery,
        hello_timeout: cfg.hello_timeout,
        control_backlog: cfg.control_backlog,
    })
}

/// Write the discovery file for a bus on `port`, if a path is given.
/// `None` when there is no path or the write failed.
fn write_discovery(path: Option<PathBuf>, port: u16) -> Option<(PathBuf, DiscoveryInfo)> {
    let path = path?;
    let info = DiscoveryInfo::for_local_bus(port);
    match discovery::write(&path, &info) {
        Ok(()) => Some((path, info)),
        Err(e) => {
            let msg = format!("cannot write {}: {e}; hosting anyway", path.display());
            crate::debug_log::server_log("bus", &msg);
            None
        }
    }
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

/// What the reader does after a frame.
enum Next {
    Read,
    Registered,
    Close,
}

/// Read and handle frames until the peer closes, the stream breaks, or
/// the bus stops. Every frame that arrived before EOF is handled first.
/// A connection that has not completed Hello by `hello_timeout` after
/// accept is closed, whatever else it sends. Returns whether the
/// connection ever completed Hello.
fn read_requests(shared: &Arc<Shared>, stream: &TcpStream, conn: ConnId) -> bool {
    let mut registered = false;
    let hello_deadline = Instant::now() + shared.hello_timeout;
    let mut reader = stream;
    while !shared.stopping() {
        if !registered && !arm_hello_timeout(stream, hello_deadline) {
            break;
        }
        let env = match read_frame(&mut reader) {
            Ok(Some(env)) => env,
            Err(FrameError::Decode(_)) => continue,
            Ok(None) | Err(_) => break,
        };
        match apply(shared, stream, conn, handle_frame(shared, conn, env)) {
            Next::Read => {}
            Next::Registered => registered = true,
            Next::Close => break,
        }
    }
    registered
}

/// Bound the next read by the time left until `deadline`. False once it
/// has passed.
fn arm_hello_timeout(stream: &TcpStream, deadline: Instant) -> bool {
    let left = deadline.saturating_duration_since(Instant::now());
    !left.is_zero() && stream.set_read_timeout(Some(left)).is_ok()
}

fn apply(shared: &Arc<Shared>, stream: &TcpStream, conn: ConnId, action: Option<Action>) -> Next {
    match action {
        Some(Action::Registered) => {
            let _ = stream.set_read_timeout(None);
            match start_writer(shared, stream, conn) {
                Ok(()) => Next::Registered,
                Err(_) => Next::Close,
            }
        }
        Some(Action::Reject(bytes)) => {
            let _ = (&*stream).write_all(&bytes);
            Next::Close
        }
        Some(Action::Shutdown) => {
            finish_shutdown(shared);
            Next::Close
        }
        Some(Action::Continue) | None => Next::Read,
    }
}

fn handle_frame(shared: &Shared, conn: ConnId, env: Envelope) -> Option<Action> {
    let msg = env.msg?;
    let mut state = shared.lock();
    // Back-pressure on requests: a client that keeps asking but never
    // reads its replies stops being read until its writer catches up. A
    // writer that fails forgets the connection, which ends this wait.
    while state.queued_control(conn) >= shared.control_backlog && !shared.stopping() {
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
/// write closes the socket and forgets the connection at once: its reader
/// may be parked on back-pressure and would otherwise never reach EOF.
fn write_frames(shared: &Shared, mut out: TcpStream, conn: ConnId) {
    loop {
        let Some(frame) = next_frame(shared, conn) else {
            return;
        };
        if out.write_all(&frame.bytes).is_err() {
            let _ = out.shutdown(NetShutdown::Both);
            shared.lock().disconnect(conn, now_ms());
            shared.wake.notify_all();
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
