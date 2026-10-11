// Bus discovery and `psmux bus` argument tests for src/bus/host.rs. Buses
// run on ephemeral ports; discovery files live in temp directories.

use super::*;
use crate::bus::discovery::DiscoveryInfo;
use crate::bus::server::BusHandle;

fn temp_discovery(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("psmux-bus-host-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("server")
}

fn bus() -> BusHandle {
    start(BusConfig::new("127.0.0.1:0".parse().unwrap(), None)).unwrap()
}

/// A port with nothing listening on it.
fn dead_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

#[test]
fn parse_args_reads_port_and_discovery() {
    let loc = BusLocation {
        port: 4321,
        discovery_path: None,
    };
    let loc = parse_args(&["--port", "5555", "--discovery", "x/server"], loc).unwrap();
    assert_eq!(loc.port, 5555);
    assert_eq!(loc.discovery_path, Some(PathBuf::from("x/server")));
}

#[test]
fn parse_args_rejects_unknown_words() {
    let loc = BusLocation {
        port: 4321,
        discovery_path: None,
    };
    assert!(parse_args(&["--bogus"], loc.clone()).is_err());
    assert!(parse_args(&["--port", "notaport"], loc).is_err());
}

#[test]
fn candidates_put_the_discovery_address_first() {
    let path = temp_discovery("order");
    discovery::write(&path, &DiscoveryInfo::for_local_bus(6001)).unwrap();
    let loc = BusLocation {
        port: 6002,
        discovery_path: Some(path),
    };
    let ports: Vec<u16> = loc.candidates().iter().map(|a| a.port()).collect();
    assert_eq!(ports, [6001, 6002]);
}

#[test]
fn live_bus_named_by_discovery_file_is_found() {
    let h = bus();
    let path = temp_discovery("live");
    discovery::write(&path, &DiscoveryInfo::for_local_bus(h.local_addr().port())).unwrap();
    let loc = BusLocation {
        port: dead_port(),
        discovery_path: Some(path),
    };
    assert_eq!(find_live_bus(&loc, PROBE_TIMEOUT), Some(h.local_addr()));
    h.stop();
    h.join();
}

#[test]
fn stale_discovery_file_falls_back_to_the_port() {
    let h = bus();
    let path = temp_discovery("stale");
    discovery::write(&path, &DiscoveryInfo::for_local_bus(dead_port())).unwrap();
    let loc = BusLocation {
        port: h.local_addr().port(),
        discovery_path: Some(path),
    };
    assert_eq!(find_live_bus(&loc, PROBE_TIMEOUT), Some(h.local_addr()));
    h.stop();
    h.join();
}

#[test]
fn nothing_live_finds_nothing() {
    let loc = BusLocation {
        port: dead_port(),
        discovery_path: Some(temp_discovery("none")),
    };
    assert_eq!(find_live_bus(&loc, Duration::from_millis(300)), None);
}

#[test]
fn run_bus_process_defers_to_a_live_bus() {
    let h = bus();
    let path = temp_discovery("defer");
    let loc = BusLocation {
        port: h.local_addr().port(),
        discovery_path: Some(path.clone()),
    };
    run_bus_process(&loc).unwrap();
    assert!(!path.exists(), "a deferring bus writes no discovery file");
    h.stop();
    h.join();
}
