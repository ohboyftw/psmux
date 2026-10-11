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

#[test]
fn test_sweep_when_port_file_already_removed_then_returns_at_once() {
    let dir = sweep_dir("graceful");
    let (path, contents) = seed_set(&dir, "exited", free_port());
    std::fs::remove_file(&path).expect("server removed its .port");
    let started = std::time::Instant::now();
    sweep_dead_registry_sets(&[(path, contents)]);
    assert!(started.elapsed() < Duration::from_millis(150));
    assert!(dir.join("exited.key").exists());
}

fn route<'a>(
    dir: &'a std::path::Path,
    env_target: &'a str,
    explicit: Option<&'a str>,
    tmux: Option<&'a str>,
) -> RouteEnv<'a> {
    RouteEnv {
        dir,
        env_target: Some(env_target),
        explicit,
        tmux,
    }
}

#[test]
fn test_route_when_inherited_warm_then_tmux_port_names_claimed_session() {
    let dir = sweep_dir("route_tmux_hit");
    seed_set(&dir, "work", 41001);
    std::thread::sleep(Duration::from_millis(20));
    seed_set(&dir, "other", 41002);
    let tmux = "/tmp/psmux-1/default,41001,0";
    let r = route(&dir, "__warm__", None, Some(tmux));
    let (name, port, _) = resolve_server_in("kill-session\n", &r).expect("routed");
    assert_eq!((name.as_str(), port), ("work", 41001));
}

#[test]
fn test_route_when_inherited_warm_and_no_port_matches_tmux_then_err() {
    let dir = sweep_dir("route_tmux_miss");
    seed_set(&dir, "work", 41001);
    let r = route(&dir, "__warm__", None, Some("/tmp/psmux-1/default,41999,0"));
    let err = resolve_server_in("kill-session\n", &r).expect_err("must not guess");
    assert!(err.to_string().contains("'__warm__'"), "{err}");
}

#[test]
fn test_route_when_inherited_warm_and_tmux_port_is_warm_then_err() {
    let dir = sweep_dir("route_tmux_warm");
    seed_set(&dir, "__warm__", 41003);
    seed_set(&dir, "work", 41001);
    let r = route(&dir, "__warm__", None, Some("/tmp/psmux-1/default,41003,0"));
    assert!(resolve_server_in("send-keys x\n", &r).is_err());
}

#[test]
fn test_route_when_explicit_warm_and_no_standby_then_err_naming_warm() {
    let dir = sweep_dir("route_explicit_warm");
    let holder = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = holder.local_addr().expect("addr").port();
    seed_set(&dir, "work", port);
    let tmux = format!("/tmp/psmux-1/default,{port},0");
    let r = route(&dir, "__warm__", Some("__warm__"), Some(&tmux));
    let err = resolve_server_in("kill-session\n", &r).expect_err("must not reach work");
    assert!(err.to_string().contains("'__warm__'"), "{err}");
}

#[test]
fn test_route_when_explicit_warm_standby_registered_then_routed_to_it() {
    let dir = sweep_dir("route_explicit_warm_live");
    seed_set(&dir, "__warm__", 41004);
    seed_set(&dir, "work", 41001);
    let r = route(&dir, "__warm__", Some("__warm__"), None);
    let (name, _, _) = resolve_server_in("kill-session\n", &r).expect("routed");
    assert_eq!(name, "__warm__");
}
