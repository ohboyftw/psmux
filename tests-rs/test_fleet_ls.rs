//! Integration test for `psmux fleet ls --json`.

use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn psmux_bin() -> &'static str {
    env!("CARGO_BIN_EXE_psmux")
}

#[test]
fn fleet_ls_json_reads_state_and_emits_workspaces() {
    let td = TempDir::new().expect("create tempdir");
    let state_path = td.path().join("state.json");
    fs::write(
        &state_path,
        include_str!("../crates/psmux-fleet/fixtures/state_healthy.json"),
    )
    .unwrap();

    let out = Command::new(psmux_bin())
        .args([
            "fleet",
            "ls",
            "--state-path",
            state_path.to_str().unwrap(),
            "--json",
        ])
        .env_remove("PSMUX_SESSION")
        .output()
        .expect("run psmux");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let parsed: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("parse JSON output");
    let workspaces = parsed
        .get("workspaces")
        .and_then(|v| v.as_array())
        .expect("workspaces array");
    assert_eq!(workspaces.len(), 3);
    assert_eq!(parsed.get("total").and_then(|v| v.as_u64()), Some(3));

    // Also verify projection correctness — not just count. Catches bugs where
    // the right number of rows ship with wrong content.
    let ids: Vec<&str> = workspaces
        .iter()
        .filter_map(|w| w.get("id").and_then(|v| v.as_str()))
        .collect();
    assert!(ids.contains(&"fix-auth"), "expected fix-auth id; got {ids:?}");
    assert!(ids.contains(&"lint-docs"));
    assert!(ids.contains(&"dead-worker"));

    // TempDir drops here, cleaning up on success or panic.
}
