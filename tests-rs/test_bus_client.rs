// Blocking client tests for src/bus/client.rs against a hand-rolled fake
// server on an ephemeral loopback port.

use super::*;
use std::net::TcpListener;
use std::thread;

/// A server that answers Hello with `reply`, then runs `after`.
fn fake_server(
    reply: Msg,
    after: impl FnOnce(TcpStream) + Send + 'static,
) -> (SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let h = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let hello = read_frame(&mut s).unwrap().unwrap();
        assert!(matches!(hello.msg, Some(Msg::Hello(_))));
        write_frame(&mut s, &Envelope { msg: Some(reply) }).unwrap();
        after(s);
    });
    (addr, h)
}

fn ack(r: &str) -> Msg {
    Msg::Ack(Ack { r#ref: r.into() })
}

const T: Duration = Duration::from_secs(5);

#[test]
fn connect_succeeds_on_hello_ack() {
    let (addr, h) = fake_server(ack("hello"), drop);
    assert!(Client::connect(addr, "c", T).is_ok());
    h.join().unwrap();
}

#[test]
fn connect_reports_an_error_reply_as_rejected() {
    let refusal = Msg::Error(super::super::proto::Error {
        message: "Server at max connections".into(),
    });
    let (addr, h) = fake_server(refusal, drop);
    let err = Client::connect(addr, "c", T).unwrap_err();
    assert!(matches!(err, ClientError::Rejected(m) if m == "Server at max connections"));
    h.join().unwrap();
}

#[test]
fn peer_closed_is_false_while_the_server_is_up() {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let (addr, h) = fake_server(ack("hello"), move |_s| {
        let _ = rx.recv();
    });
    let c = Client::connect(addr, "c", T).unwrap();
    assert!(!c.peer_closed());
    tx.send(()).unwrap();
    h.join().unwrap();
}

#[test]
fn peer_closed_is_true_after_the_server_closes() {
    let (addr, h) = fake_server(ack("hello"), drop);
    let c = Client::connect(addr, "c", T).unwrap();
    h.join().unwrap();
    let deadline = std::time::Instant::now() + T;
    while !c.peer_closed() && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(c.peer_closed());
}

#[test]
fn peer_closed_leaves_the_socket_blocking() {
    let (addr, h) = fake_server(ack("hello"), |mut s| {
        thread::sleep(Duration::from_millis(100));
        let ev = Msg::Event(Event::default());
        write_frame(&mut s, &Envelope { msg: Some(ev) }).unwrap();
    });
    let mut c = Client::connect(addr, "c", T).unwrap();
    assert!(!c.peer_closed());
    assert!(c.next_event().is_ok());
    h.join().unwrap();
}

#[test]
fn probe_is_false_when_nothing_listens() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    assert!(!probe(addr, Duration::from_millis(300)));
}
