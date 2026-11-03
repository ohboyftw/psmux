// Publisher tests for src/mycel.rs against an in-process bus on an
// ephemeral port. Autostart is always off here: these tests never spawn
// a `psmux bus` process and never touch ~/.mycel/server.

use super::*;
use crate::bus::server::{start, BusConfig, BusHandle};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

const T: Duration = Duration::from_secs(5);
/// Port 0: a connect to it fails at once, and no other test can take it.
const NO_PORT: u16 = 0;

fn temp_discovery(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("psmux-mycel-pub-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("server")
}

/// An instant `ago` before now (a little more, to clear `>=` checks).
fn past(ago: Duration) -> Instant {
    Instant::now()
        .checked_sub(ago + Duration::from_secs(1))
        .unwrap()
}

/// Host again on a port just freed. Retried, because a parallel test may
/// briefly hold it.
fn rebind(port: u16) -> BusHandle {
    let deadline = Instant::now() + T;
    loop {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        match start(BusConfig::new(addr, None)) {
            Ok(h) => return h,
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("cannot rebind {port}: {e}"),
        }
    }
}

fn bus_on(port: u16) -> BusHandle {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    start(BusConfig::new(addr, None)).unwrap()
}

fn location(port: u16) -> BusLocation {
    BusLocation {
        port,
        discovery_path: None,
    }
}

fn subscriber(h: &BusHandle) -> Client {
    let mut c = Client::connect(h.local_addr(), "sub", T).unwrap();
    c.set_read_timeout(Some(T)).unwrap();
    c.subscribe("psmux/#").unwrap();
    c
}

fn msg(topic: &str) -> PublishMsg {
    PublishMsg {
        topic: topic.into(),
        payload: b"{}".to_vec(),
    }
}

#[test]
fn link_publishes_through_a_live_bus() {
    let h = bus_on(0);
    let mut sub = subscriber(&h);
    let mut link = Link::new("psmux@TEST", location(h.local_addr().port()), None);
    link.publish(&msg(topics::PANE_CREATED));
    let e = sub.next_event().unwrap();
    assert_eq!(
        (e.topic.as_str(), e.publisher.as_str()),
        (topics::PANE_CREATED, "psmux@TEST")
    );
    h.stop();
    h.join();
}

#[test]
fn link_reconnects_to_a_restarted_bus_without_losing_the_next_event() {
    let first = bus_on(0);
    let port = first.local_addr().port();
    let mut link = Link::new("psmux@TEST", location(port), None);
    link.publish(&msg("psmux/one"));
    first.stop();
    first.join();
    let second = rebind(port);
    let mut sub = subscriber(&second);
    // The old socket is closed; peer_closed() must notice before writing.
    let deadline = Instant::now() + T;
    while link.client.as_ref().is_some_and(|c| !c.peer_closed()) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    link.publish(&msg("psmux/two"));
    assert_eq!(sub.next_event().unwrap().topic, "psmux/two");
    second.stop();
    second.join();
}

#[test]
fn link_backs_off_after_a_failed_connect() {
    let mut link = Link::new("psmux@TEST", location(NO_PORT), None);
    link.publish(&msg("psmux/lost"));
    assert!(link.client.is_none());
    assert!(link.retry_at.is_some_and(|t| t > Instant::now()));
    let started = Instant::now();
    link.publish(&msg("psmux/lost"));
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "no connect during backoff"
    );
}

#[test]
fn autostart_is_off_for_a_test_harness() {
    // The test binary is not psmux, so it must never be spawned as a bus.
    assert_eq!(autostart_exe(std::env::current_exe().ok()), None);
}

#[test]
fn autostart_uses_a_psmux_image() {
    let exe = PathBuf::from("bin/psmux.exe");
    assert_eq!(autostart_exe(Some(exe.clone())), Some(exe));
}

#[test]
fn start_and_connect_connects_to_the_started_bus() {
    // Nothing answers before the start: the port is held by a listener
    // that never accepts, and the discovery file does not exist yet.
    let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let path = temp_discovery("autostart");
    let loc = BusLocation {
        port: held.local_addr().unwrap().port(),
        discovery_path: Some(path.clone()),
    };
    let hosted: Arc<Mutex<Option<BusHandle>>> = Arc::default();
    let slot = hosted.clone();
    let spawn: Spawner = Box::new(move |_: &BusLocation| {
        let cfg = BusConfig::new("127.0.0.1:0".parse().unwrap(), Some(path.clone()));
        *slot.lock().unwrap() = Some(start(cfg)?);
        Ok(())
    });
    let mut link = Link::new("psmux@TEST", loc, Some(spawn));
    link.publish(&msg("psmux/first"));
    assert!(link.client.is_some());
    let h = hosted.lock().unwrap().take().unwrap();
    h.stop();
    h.join();
}

#[test]
fn start_attempts_stop_after_repeated_failures() {
    let calls = Arc::new(AtomicU32::new(0));
    let counter = calls.clone();
    let spawn: Spawner = Box::new(move |_: &BusLocation| {
        counter.fetch_add(1, Ordering::SeqCst);
        Err(io::Error::other("cannot start"))
    });
    let mut link = Link::new("psmux@TEST", location(NO_PORT), Some(spawn));
    for _ in 0..(MAX_FAILED_STARTS + 2) {
        link.retry_at = None;
        link.last_spawn = link.last_spawn.map(|_| past(RESPAWN_INTERVAL));
        link.publish(&msg("psmux/lost"));
    }
    assert_eq!(calls.load(Ordering::SeqCst), MAX_FAILED_STARTS);
}

#[test]
fn mycel_bus_publish_never_blocks_without_a_bus() {
    let bus = MycelBus::with_link(Link::new("psmux@TEST", location(NO_PORT), None));
    let started = Instant::now();
    for _ in 0..(QUEUE_CAPACITY * 2) {
        bus.publish("psmux/flood", &serde_json::json!({ "n": 1 }));
    }
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(bus.client_id(), "psmux@TEST");
}

#[test]
fn mycel_bus_publishes_to_a_live_bus_when_enabled() {
    let h = bus_on(0);
    let mut sub = subscriber(&h);
    let bus = MycelBus::with_env("psmux@TEST", None, location(h.local_addr().port()), None);
    bus.publish(topics::PANE_CREATED, &serde_json::json!({}));
    assert_eq!(sub.next_event().unwrap().topic, topics::PANE_CREATED);
    h.stop();
    h.join();
}

#[test]
fn mycel_bus_never_publishes_when_psmux_bus_is_zero() {
    let h = bus_on(0);
    let mut sub = subscriber(&h);
    sub.set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let bus = MycelBus::with_env(
        "psmux@TEST",
        Some(" 0 "),
        location(h.local_addr().port()),
        None,
    );
    bus.publish(topics::PANE_CREATED, &serde_json::json!({}));
    assert!(
        sub.next_event().is_err(),
        "PSMUX_BUS=0 must not reach a live bus"
    );
    assert_eq!(bus.client_id(), "psmux@TEST");
    h.stop();
    h.join();
}
