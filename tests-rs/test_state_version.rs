//! Contract: state.json has a monotonic `state_version` field that increments
//! on every write and defaults to 0 when loading pre-existing files.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn unique_tmp_dir(tag: &str) -> PathBuf {
    let ts = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "psmux_state_version_{}_{}_{}",
        tag,
        std::process::id(),
        ts
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn state_version_defaults_to_zero_on_old_files() {
    let dir = unique_tmp_dir("default");
    let path = dir.join("state.json");
    // Write a pre-existing state.json without state_version.
    fs::write(&path, r#"{"session":"demo","workers":{}}"#).unwrap();
    let state: psmux::orchestrate::OrchestrationState =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state.state_version, 0);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn state_version_serializes_when_present() {
    let dir = unique_tmp_dir("serialize");
    let path = dir.join("state.json");
    let state = psmux::orchestrate::OrchestrationState {
        session: "demo".into(),
        workers: HashMap::new(),
        state_version: 42,
    };
    fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("\"state_version\":42"),
        "state.json missing state_version: {text}"
    );
    fs::remove_dir_all(&dir).ok();
}
