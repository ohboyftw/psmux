// Kill marker and warm standby verdict (ported by intent from upstream 99bfd9e).
use super::*;

fn fresh_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("psmux_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn facts() -> WarmStandbyFacts {
    WarmStandbyFacts {
        own_creation: Some(1_000),
        spawner_creation: Some(500),
        spawner_alive: true,
        kill_marker: None,
        data_dir_present: true,
    }
}

#[test]
fn test_verdict_when_no_marker_then_keep() {
    assert_eq!(warm_standby_verdict(&facts()), WarmStandbyVerdict::Keep);
}

#[test]
fn test_verdict_when_data_dir_gone_then_data_dir_gone() {
    let f = WarmStandbyFacts {
        data_dir_present: false,
        kill_marker: Some(1),
        ..facts()
    };
    assert_eq!(warm_standby_verdict(&f), WarmStandbyVerdict::DataDirGone);
}

#[test]
fn test_verdict_when_marker_at_own_creation_then_namespace_killed() {
    let f = WarmStandbyFacts {
        kill_marker: Some(1_000),
        ..facts()
    };
    assert_eq!(
        warm_standby_verdict(&f),
        WarmStandbyVerdict::NamespaceKilled
    );
}

#[test]
fn test_verdict_when_marker_older_than_standby_and_spawner_alive_then_keep() {
    let f = WarmStandbyFacts {
        kill_marker: Some(900),
        ..facts()
    };
    assert_eq!(warm_standby_verdict(&f), WarmStandbyVerdict::Keep);
}

#[test]
fn test_verdict_when_spawner_dead_after_newer_kill_then_spawner_killed() {
    let f = WarmStandbyFacts {
        kill_marker: Some(900),
        spawner_alive: false,
        ..facts()
    };
    assert_eq!(warm_standby_verdict(&f), WarmStandbyVerdict::SpawnerKilled);
}

#[test]
fn test_verdict_when_spawner_dead_but_kill_predates_it_then_keep() {
    let f = WarmStandbyFacts {
        kill_marker: Some(400),
        spawner_alive: false,
        ..facts()
    };
    assert_eq!(warm_standby_verdict(&f), WarmStandbyVerdict::Keep);
}

#[test]
fn test_verdict_when_times_unknown_then_keep() {
    let f = WarmStandbyFacts {
        own_creation: None,
        spawner_creation: None,
        spawner_alive: false,
        kill_marker: Some(u64::MAX),
        data_dir_present: true,
    };
    assert_eq!(warm_standby_verdict(&f), WarmStandbyVerdict::Keep);
}

#[test]
fn test_read_marker_when_none_written_then_none() {
    let dir = fresh_dir("marker_none");
    assert_eq!(read_kill_marker(&dir, Some("ns")), None);
    assert_eq!(read_kill_marker(&dir, None), None);
}

#[test]
fn test_read_marker_when_all_is_newer_then_all_wins() {
    let dir = fresh_dir("marker_all_newer");
    write_kill_marker(&dir, KillScope::Namespace("ns"), 10);
    write_kill_marker(&dir, KillScope::All, 20);
    assert_eq!(read_kill_marker(&dir, Some("ns")), Some(20));
}

#[test]
fn test_read_marker_when_namespace_is_newer_then_namespace_wins() {
    let dir = fresh_dir("marker_ns_newer");
    write_kill_marker(&dir, KillScope::Namespace("ns"), 30);
    write_kill_marker(&dir, KillScope::All, 20);
    assert_eq!(read_kill_marker(&dir, Some("ns")), Some(30));
}

#[test]
fn test_read_marker_when_other_namespace_killed_then_none() {
    let dir = fresh_dir("marker_other_ns");
    write_kill_marker(&dir, KillScope::Namespace("other"), 30);
    assert_eq!(read_kill_marker(&dir, Some("ns")), None);
}

#[test]
fn test_read_marker_when_default_namespace_then_only_all_counts() {
    let dir = fresh_dir("marker_default_ns");
    write_kill_marker(&dir, KillScope::Namespace("ns"), 30);
    assert_eq!(read_kill_marker(&dir, None), None);
    write_kill_marker(&dir, KillScope::All, 5);
    assert_eq!(read_kill_marker(&dir, None), Some(5));
}

#[test]
fn test_marker_path_when_namespace_named_all_then_distinct_from_everything() {
    let dir = PathBuf::from("d");
    assert_ne!(
        kill_marker_path(&dir, KillScope::Namespace("all")),
        kill_marker_path(&dir, KillScope::All)
    );
}

#[test]
fn test_write_marker_when_rewritten_then_latest_stamp_and_no_temp_left() {
    let dir = fresh_dir("marker_rewrite");
    write_kill_marker(&dir, KillScope::All, 1);
    write_kill_marker(&dir, KillScope::All, 2);
    assert_eq!(read_kill_marker(&dir, None), Some(2));
    let left: Vec<_> = std::fs::read_dir(dir.join("killed"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(left.len(), 1);
}

#[test]
fn test_spawner_spec_when_round_tripped_then_identity_preserved() {
    assert_eq!(
        parse_spawner(&format_spawner(42, 133_000_000_000_000_000)),
        Some((42, 133_000_000_000_000_000))
    );
}

#[test]
fn test_spawner_spec_when_zero_or_garbage_then_rejected() {
    assert_eq!(parse_spawner("0:5"), None);
    assert_eq!(parse_spawner("5:0"), None);
    assert_eq!(parse_spawner("abc"), None);
    assert_eq!(parse_spawner("5:x"), None);
}

#[cfg(windows)]
#[test]
fn test_own_creation_when_queried_then_precedes_now() {
    let own = process_creation_time(std::process::id()).expect("own creation time");
    assert!(own <= now_filetime());
    assert!(process_is_alive(std::process::id()));
}
