// Publisher tests for src/mycel.rs against an in-process bus on an
// ephemeral port. Autostart is always off here: these tests never spawn
// a `psmux bus` process and never touch ~/.mycel/server.

use super::*;
use crate::bus::server::{start, BusConfig, BusHandle};

const T: Duration = Duration::from_secs(5);

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
    let second = bus_on(port);
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
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let mut link = Link::new("psmux@TEST", location(port), None);
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
    assert_eq!(autostart_exe(), None);
}

#[test]
fn mycel_bus_publish_never_blocks_without_a_bus() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let bus = MycelBus::with_link(Link::new("psmux@TEST", location(port), None));
    let started = Instant::now();
    for _ in 0..(QUEUE_CAPACITY * 2) {
        bus.publish("psmux/flood", &serde_json::json!({ "n": 1 }));
    }
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(bus.client_id(), "psmux@TEST");
}
