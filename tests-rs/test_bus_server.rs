// End-to-end tests for src/bus/server.rs: a bus on an ephemeral
// 127.0.0.1 port (never 4321) driven through the blocking client. The
// discovery file, when used, lives in a temp directory.

use super::*;
use crate::bus::client::{Client, ClientError};
use crate::bus::proto::Msg;
use std::io::Read;

const T: Duration = Duration::from_secs(5);

fn config() -> BusConfig {
    BusConfig::new("127.0.0.1:0".parse().unwrap(), None)
}

fn bus() -> BusHandle {
    start(config()).unwrap()
}

fn client(h: &BusHandle, id: &str) -> Client {
    let c = Client::connect(h.local_addr(), id, T).unwrap();
    c.set_read_timeout(Some(T)).unwrap();
    c
}

fn temp_discovery(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("psmux-bus-srv-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("server")
}

#[test]
fn pub_sub_round_trip() {
    let h = bus();
    let mut sub = client(&h, "sub");
    sub.subscribe("test/#").unwrap();
    let mut publ = client(&h, "test-pub");
    publ.publish("test/hello", b"world", HashMap::new())
        .unwrap();
    let e = sub.next_event().unwrap();
    assert_eq!(
        (e.topic.as_str(), e.payload.as_slice()),
        ("test/hello", &b"world"[..])
    );
    assert_eq!((e.sequence, e.publisher.as_str()), (1, "test-pub"));
    h.stop();
    h.join();
}

#[test]
fn publish_then_immediate_close_is_still_delivered_and_stored() {
    let h = bus();
    let mut sub = client(&h, "sub");
    sub.subscribe("fire/#").unwrap();
    let mut publ = client(&h, "oneshot");
    publ.publish("fire/and/forget", b"x", HashMap::new())
        .unwrap();
    publ.close();
    assert_eq!(sub.next_event().unwrap().topic, "fire/and/forget");
    let mut late = client(&h, "late");
    assert_eq!(late.replay("fire/#", 0, 0).unwrap().len(), 1);
    h.stop();
    h.join();
}

#[test]
fn presence_connected_then_disconnected_once() {
    let h = bus();
    let mut mon = client(&h, "monitor");
    mon.subscribe("$mycel/clients/+").unwrap();
    client(&h, "visitor").close();
    let topics: Vec<String> = (0..2).map(|_| mon.next_event().unwrap().topic).collect();
    assert_eq!(
        topics,
        ["$mycel/clients/connected", "$mycel/clients/disconnected"]
    );
    let st = mon.status().unwrap();
    assert_eq!(st.clients.len(), 1);
    h.stop();
    h.join();
}

#[test]
fn status_reply_is_the_next_frame() {
    let h = bus();
    let mut c = client(&h, "asker");
    let st = c.status().unwrap();
    assert_eq!(st.clients[0].client_id, "asker");
    assert!(st.topics.iter().any(|t| t.topic == "$mycel/server/started"));
    h.stop();
    h.join();
}

#[test]
fn server_started_is_replayable() {
    let h = bus();
    let mut c = client(&h, "r");
    let events = c.replay("$mycel/server/started", 0, 0).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&events[0].payload).unwrap();
    assert_eq!(v["port"], h.local_addr().port());
    h.stop();
    h.join();
}

#[test]
fn connection_without_hello_is_closed_after_the_timeout() {
    let mut cfg = config();
    cfg.hello_timeout = Duration::from_millis(200);
    let h = start(cfg).unwrap();
    let mut raw = TcpStream::connect(h.local_addr()).unwrap();
    raw.set_read_timeout(Some(T)).unwrap();
    let mut buf = [0u8; 1];
    assert_eq!(raw.read(&mut buf).unwrap_or(0), 0);
    let mut c = client(&h, "check");
    assert_eq!(c.status().unwrap().total_events, 2); // started + check's connected
    h.stop();
    h.join();
}

#[test]
fn max_connections_rejects_with_error() {
    let mut cfg = config();
    cfg.limits.max_connections = 1;
    let h = start(cfg).unwrap();
    let _first = client(&h, "first");
    let err = Client::connect(h.local_addr(), "second", T).unwrap_err();
    assert!(matches!(err, ClientError::Rejected(m) if m == "Server at max connections"));
    h.stop();
    h.join();
}

#[test]
fn heartbeat_reaches_explicit_subscribers() {
    let mut cfg = config();
    cfg.heartbeat_interval = Duration::from_millis(100);
    let h = start(cfg).unwrap();
    let mut mon = client(&h, "monitor");
    mon.subscribe("$mycel/clients/heartbeat").unwrap();
    let e = mon.next_event().unwrap();
    assert_eq!(e.publisher, "server");
    h.stop();
    h.join();
}

#[test]
fn shutdown_is_acked_and_stops_the_bus() {
    let h = bus();
    let addr = h.local_addr();
    let mut c = client(&h, "cli");
    c.shutdown("test").unwrap();
    h.join();
    assert!(Client::connect(addr, "after", Duration::from_millis(300)).is_err());
}

#[test]
fn stop_returns_with_an_idle_subscriber_connected() {
    let h = bus();
    let mut sub = client(&h, "idle");
    sub.subscribe("never/#").unwrap();
    h.stop();
    h.join();
    assert!(matches!(sub.next_msg(), Err(_)));
}

#[test]
fn discovery_file_is_written_after_bind_and_removed_on_stop() {
    let path = temp_discovery("lifecycle");
    let h = start(BusConfig::new(
        "127.0.0.1:0".parse().unwrap(),
        Some(path.clone()),
    ))
    .unwrap();
    let info = discovery::read(&path).unwrap();
    assert_eq!(
        (info.host.as_str(), info.port),
        ("127.0.0.1", h.local_addr().port())
    );
    h.stop();
    h.join();
    assert!(!path.exists());
}

#[test]
fn second_bus_cannot_bind_the_same_port() {
    let h = bus();
    let err = start(BusConfig::new(h.local_addr(), None)).err().unwrap();
    assert!(matches!(
        err.kind(),
        io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
    ));
    h.stop();
    h.join();
}

#[test]
fn unknown_server_to_client_frames_are_ignored() {
    let h = bus();
    let mut c = client(&h, "odd");
    let mut raw = TcpStream::connect(h.local_addr()).unwrap();
    let ack = Msg::Ack(crate::bus::proto::Ack { r#ref: "x".into() });
    crate::bus::codec::write_frame(&mut raw, &Envelope { msg: Some(ack) }).unwrap();
    assert!(c.status().is_ok());
    h.stop();
    h.join();
}
