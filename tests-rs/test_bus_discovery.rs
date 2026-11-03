// Discovery file tests for src/bus/discovery.rs. Every test uses its own
// temp directory; the real ~/.mycel/server is never touched.

use super::*;

fn temp_file(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("psmux-bus-disc-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir.join(".mycel").join("server")
}

#[test]
fn write_creates_the_directory_and_round_trips() {
    let path = temp_file("roundtrip");
    let info = DiscoveryInfo::for_local_bus(4321);
    write(&path, &info).unwrap();
    assert_eq!(read(&path), Some(info));
}

#[test]
fn written_file_has_string_host_and_numeric_port() {
    // The shape the C++ CLI, the Rust client and the Python clients parse.
    let path = temp_file("shape");
    write(&path, &DiscoveryInfo::for_local_bus(4321)).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["host"], "127.0.0.1");
    assert_eq!(v["port"], 4321);
    assert!(text.ends_with('\n'));
}

#[test]
fn reads_the_cpp_server_format() {
    let path = temp_file("cpp");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "{\"host\":\"127.0.0.1\",\"port\":4321,\"tailscale_ip\":\"100.64.0.1\"}\n",
    )
    .unwrap();
    let info = read(&path).unwrap();
    assert_eq!((info.port, info.pid), (4321, None));
    assert_eq!(info.tailscale_ip.as_deref(), Some("100.64.0.1"));
}

#[test]
fn unparseable_file_reads_as_none() {
    let path = temp_file("garbage");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "not json").unwrap();
    assert_eq!(read(&path), None);
}

#[test]
fn missing_file_reads_as_none() {
    assert_eq!(read(&temp_file("missing")), None);
}

#[test]
fn remove_if_ours_removes_our_file() {
    let path = temp_file("ours");
    let info = DiscoveryInfo::for_local_bus(5000);
    write(&path, &info).unwrap();
    remove_if_ours(&path, &info);
    assert!(!path.exists());
}

#[test]
fn remove_if_ours_keeps_a_file_another_host_wrote() {
    let path = temp_file("theirs");
    write(&path, &DiscoveryInfo::for_local_bus(6000)).unwrap();
    remove_if_ours(&path, &DiscoveryInfo::for_local_bus(5000));
    assert!(path.exists());
}

#[test]
fn socket_addr_resolves_localhost_to_ipv4() {
    let info = DiscoveryInfo {
        host: "localhost".into(),
        port: 4321,
        tailscale_ip: None,
        pid: None,
    };
    let addr = info.socket_addr().unwrap();
    assert!(addr.is_ipv4());
}
