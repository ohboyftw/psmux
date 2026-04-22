use psmux_fleet::{StateReader, WorkspaceStatus};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

#[test]
fn reads_state_version_from_fixture() {
    let reader = StateReader::load(&fixture("state_healthy.json")).unwrap();
    assert_eq!(reader.state_version(), 7);
    assert_eq!(reader.session(), "demo");
}

#[test]
fn maps_running_worker_to_workspace() {
    let reader = StateReader::load(&fixture("state_healthy.json")).unwrap();
    let workspaces = reader.workspaces();
    let fix_auth = workspaces.iter().find(|w| w.id == "fix-auth").unwrap();
    assert_eq!(fix_auth.status, WorkspaceStatus::Running);
    assert_eq!(fix_auth.pane_id.as_deref(), Some("%3"));
}

#[test]
fn maps_terminal_worker_with_exit_code() {
    let reader = StateReader::load(&fixture("state_healthy.json")).unwrap();
    let workspaces = reader.workspaces();
    let lint = workspaces.iter().find(|w| w.id == "lint-docs").unwrap();
    assert_eq!(lint.status, WorkspaceStatus::Succeeded);
    assert_eq!(lint.exit_code, Some(0));
    assert!(lint.finished_at.is_some());
}

#[test]
fn preserves_crash_dump_path() {
    let reader = StateReader::load(&fixture("state_healthy.json")).unwrap();
    let workspaces = reader.workspaces();
    let dead = workspaces.iter().find(|w| w.id == "dead-worker").unwrap();
    assert!(dead.crash_dump_path.is_some());
    assert_eq!(dead.status, WorkspaceStatus::Failed);
}
