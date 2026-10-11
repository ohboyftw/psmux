//! #622: a failed `has-session` probe reaps a session's registry only when the
//! server is truly gone, and then the whole set, never on a mere timeout.

use super::*;
use std::net::TcpListener;
use std::path::{Path, PathBuf};

const SET: [&str; 4] = ["port", "key", "pipe", "version"];

fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clean pre-existing temp dir");
    }
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_set(dir: &Path, base: &str, port: u16) -> PathBuf {
    for ext in SET {
        let body = if ext == "port" {
            port.to_string()
        } else {
            "x".into()
        };
        std::fs::write(dir.join(format!("{base}.{ext}")), body).expect("write fixture");
    }
    dir.join(format!("{base}.port"))
}

fn surviving(dir: &Path, base: &str) -> Vec<&'static str> {
    SET.into_iter()
        .filter(|ext| dir.join(format!("{base}.{ext}")).exists())
        .collect()
}

fn unbound_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind scratch listener");
    l.local_addr().expect("local_addr").port()
}

/// The listener never calls `accept`: the shape of a live server that is too
/// busy to answer has-session inside its connect budget.
#[test]
fn failed_probe_when_port_still_bound_then_whole_set_survives() {
    let dir = fresh_dir("psmux_622_busy_but_alive");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind busy listener");
    let port = listener.local_addr().expect("local_addr").port();
    let path = write_set(&dir, "busy", port);

    remove_registry_set_at_if_dead(&path);

    assert_eq!(surviving(&dir, "busy"), SET.to_vec());
}

#[test]
fn failed_probe_when_port_unbound_then_whole_set_is_reaped() {
    let dir = fresh_dir("psmux_622_truly_dead");
    let path = write_set(&dir, "dead", unbound_port());

    remove_registry_set_at_if_dead(&path);

    assert!(
        surviving(&dir, "dead").is_empty(),
        "left: {:?}",
        surviving(&dir, "dead")
    );
}

#[test]
fn failed_probe_when_reaping_one_session_then_peer_set_is_untouched() {
    let dir = fresh_dir("psmux_622_peer_untouched");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind peer listener");
    let peer_port = listener.local_addr().expect("local_addr").port();
    write_set(&dir, "peer", peer_port);
    let dead = write_set(&dir, "dead", unbound_port());

    remove_registry_set_at_if_dead(&dead);

    assert_eq!(surviving(&dir, "peer"), SET.to_vec());
}

#[test]
fn failed_probe_when_port_file_unreadable_as_port_then_set_is_reaped() {
    let dir = fresh_dir("psmux_622_garbage_port");
    let path = write_set(&dir, "junk", 0);
    std::fs::write(&path, "not-a-port").expect("corrupt .port");

    remove_registry_set_at_if_dead(&path);

    assert!(surviving(&dir, "junk").is_empty());
}

#[test]
fn failed_probe_when_port_file_missing_then_sidecars_are_left_to_the_sweep() {
    let dir = fresh_dir("psmux_622_no_port");
    let path = write_set(&dir, "gone", unbound_port());
    std::fs::remove_file(&path).expect("drop .port");

    remove_registry_set_at_if_dead(&path);

    assert_eq!(surviving(&dir, "gone"), vec!["key", "pipe", "version"]);
}
