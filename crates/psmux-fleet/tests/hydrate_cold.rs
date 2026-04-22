use psmux_fleet::{WorkspaceStatus, WorkspaceTable};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

#[test]
fn hydrate_cold_from_state_json() {
    let table = WorkspaceTable::hydrate_cold(&fixture("state_healthy.json")).unwrap();
    assert_eq!(table.len(), 3);
    let ids: Vec<_> = table.workspaces().iter().map(|w| w.id.as_str()).collect();
    assert!(ids.contains(&"fix-auth"));
    assert!(ids.contains(&"lint-docs"));
    assert!(ids.contains(&"dead-worker"));
}

#[test]
fn find_by_id_returns_specific_workspace() {
    let table = WorkspaceTable::hydrate_cold(&fixture("state_healthy.json")).unwrap();
    let ws = table.find("lint-docs").unwrap();
    assert_eq!(ws.status, WorkspaceStatus::Succeeded);
    assert_eq!(ws.exit_code, Some(0));
}

#[test]
fn filter_by_status_narrows_result() {
    let table = WorkspaceTable::hydrate_cold(&fixture("state_healthy.json")).unwrap();
    let running = table.filter_status(WorkspaceStatus::Running);
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].id, "fix-auth");
}
