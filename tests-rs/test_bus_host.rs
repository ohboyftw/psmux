// Bus discovery and `psmux bus` argument tests for src/bus/host.rs. Buses
// run on ephemeral ports; discovery files live in temp directories.

use super::*;
use crate::bus::discovery::DiscoveryInfo;
use crate::bus::server::BusHandle;
use std::net::SocketAddr;

fn temp_discovery(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("psmux-bus-host-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("server")
}

fn bus() -> BusHandle {
    start(BusConfig::new("127.0.0.1:0".parse().unwrap(), None)).unwrap()
}

/// Port 0: a connect to it fails at once, and no other test can take it
/// (a freed ephemeral port could be reused by a parallel test).
const NO_PORT: u16 = 0;

/// Run `run_bus_process` on a thread and wait until its discovery file
/// names a live port other than `NO_PORT`.
fn host_on_thread(loc: BusLocation) -> (std::thread::JoinHandle<std::io::Result<()>>, SocketAddr) {
    let path = loc.discovery_path.clone().unwrap();
    let worker = std::thread::spawn(move || run_bus_process(&loc));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let found = discovery::read(&path).filter(|i| i.port != NO_PORT);
        if let Some(addr) = found.and_then(|i| i.socket_addr()) {
            return (worker, addr);
        }
        assert!(std::time::Instant::now() < deadline, "bus never hosted");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn shut_down(addr: SocketAddr) {
    let mut c = Client::connect(addr, "test", Duration::from_secs(5)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    c.shutdown("test").unwrap();
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
        port: NO_PORT,
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
    discovery::write(&path, &DiscoveryInfo::for_local_bus(NO_PORT)).unwrap();
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
        port: NO_PORT,
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

#[test]
fn run_bus_process_hosts_until_shutdown() {
    let path = temp_discovery("host");
    let loc = BusLocation {
        port: NO_PORT,
        discovery_path: Some(path.clone()),
    };
    let (worker, addr) = host_on_thread(loc);
    shut_down(addr);
    assert!(worker.join().unwrap().is_ok());
    assert!(!path.exists(), "the host removes its discovery file");
}

#[test]
fn run_bus_process_overwrites_a_stale_discovery_file() {
    let path = temp_discovery("overwrite");
    let stale = DiscoveryInfo {
        host: "127.0.0.1".into(),
        port: NO_PORT,
        tailscale_ip: None,
        pid: Some(1),
    };
    discovery::write(&path, &stale).unwrap();
    let loc = BusLocation {
        port: NO_PORT,
        discovery_path: Some(path.clone()),
    };
    let (worker, addr) = host_on_thread(loc);
    assert_eq!(
        discovery::read(&path).unwrap().pid,
        Some(std::process::id())
    );
    shut_down(addr);
    assert!(worker.join().unwrap().is_ok());
}

#[test]
fn run_bus_process_returns_ok_when_a_non_mycel_listener_holds_the_port() {
    let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let path = temp_discovery("held");
    let loc = BusLocation {
        port: held.local_addr().unwrap().port(),
        discovery_path: Some(path.clone()),
    };
    assert!(run_bus_process(&loc).is_ok());
    assert!(!path.exists(), "a bus that lost the port writes nothing");
}

#[test]
fn stop_live_bus_shuts_the_bus_down() {
    let h = bus();
    let loc = BusLocation {
        port: h.local_addr().port(),
        discovery_path: None,
    };
    assert!(stop_live_bus(&loc).unwrap());
    h.join();
}

#[test]
fn stop_live_bus_reports_no_bus() {
    let loc = BusLocation {
        port: NO_PORT,
        discovery_path: None,
    };
    assert!(!stop_live_bus(&loc).unwrap());
}

#[test]
fn bus_cwd_is_not_the_callers_directory() {
    let exe = std::env::current_exe().unwrap();
    let cwd = bus_cwd(&exe).unwrap();
    assert!(cwd.is_dir());
    assert_ne!(cwd, std::env::current_dir().unwrap());
}
