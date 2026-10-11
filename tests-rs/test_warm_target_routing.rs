// A hidden-session name given with -t must never be re-routed to another
// server, and the post-kill registry sweep must only touch dead, unchanged sets.
use super::*;

#[test]
fn test_env_route_when_explicit_warm_then_not_redirected() {
    assert!(!env_target_redirects("__warm__", Some("__warm__")));
}

#[test]
fn test_env_route_when_explicit_namespaced_warm_then_not_redirected() {
    assert!(!env_target_redirects("ns____warm__", Some("ns____warm__")));
}

#[test]
fn test_env_route_when_explicit_pooled_warm_then_not_redirected() {
    assert!(!env_target_redirects("__warm__3", Some("__warm__3")));
}

#[test]
fn test_env_route_when_warm_inherited_then_redirected() {
    assert!(env_target_redirects("__warm__", None));
}

#[test]
fn test_env_route_when_explicit_names_other_session_then_inherited_warm_redirected() {
    assert!(env_target_redirects("__warm__", Some("work")));
}

#[test]
fn test_env_route_when_real_session_then_not_redirected() {
    assert!(!env_target_redirects("work", Some("work")));
    assert!(!env_target_redirects("work", None));
}

fn sweep_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("psmux_sweep_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn seed_set(dir: &std::path::Path, base: &str, port: u16) -> (std::path::PathBuf, String) {
    let path = dir.join(format!("{base}.port"));
    std::fs::write(&path, port.to_string()).expect("seed port");
    std::fs::write(dir.join(format!("{base}.key")), "k").expect("seed key");
    (path, port.to_string())
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    l.local_addr().expect("addr").port()
}

#[test]
fn test_sweep_when_port_unbound_then_set_removed() {
    let dir = sweep_dir("dead");
    let entry = seed_set(&dir, "gone", free_port());
    sweep_dead_registry_sets(&[entry]);
    assert!(!dir.join("gone.port").exists());
    assert!(!dir.join("gone.key").exists());
}

#[test]
fn test_sweep_when_port_still_bound_then_set_kept() {
    let dir = sweep_dir("live");
    let holder = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let entry = seed_set(&dir, "alive", holder.local_addr().expect("addr").port());
    sweep_dead_registry_sets(&[entry]);
    assert!(dir.join("alive.port").exists());
}

#[test]
fn test_sweep_when_port_file_rewritten_since_kill_then_set_kept() {
    let dir = sweep_dir("rewritten");
    let (path, _) = seed_set(&dir, "reborn", free_port());
    sweep_dead_registry_sets(&[(path, "1".to_string())]);
    assert!(dir.join("reborn.port").exists());
}
