//! Integration test for `psmux fleet status <id>`.

use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn psmux_bin() -> &'static str {
    env!("CARGO_BIN_EXE_psmux")
}

#[test]
fn fleet_status_emits_workspace_detail_json() {
    let td = TempDir::new().expect("create tempdir");
    let state = td.path().join("state.json");
    fs::write(
        &state,
        include_str!("../crates/psmux-fleet/fixtures/state_healthy.json"),
    )
    .unwrap();

    let out = Command::new(psmux_bin())
        .args([
            "fleet",
            "status",
            "lint-docs",
            "--state-path",
            state.to_str().unwrap(),
            "--json",
        ])
        .env_remove("PSMUX_SESSION")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["id"], "lint-docs");
    assert_eq!(v["status"], "succeeded");
    assert_eq!(v["exit_code"], 0);
}

#[test]
fn fleet_status_unknown_id_errors() {
    let td = TempDir::new().expect("create tempdir");
    let state = td.path().join("state.json");
    fs::write(
        &state,
        include_str!("../crates/psmux-fleet/fixtures/state_healthy.json"),
    )
    .unwrap();

    let out = Command::new(psmux_bin())
        .args([
            "fleet",
            "status",
            "nonexistent",
            "--state-path",
            state.to_str().unwrap(),
            "--json",
        ])
        .env_remove("PSMUX_SESSION")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("WORKSPACE_NOT_FOUND") || stderr.contains("nonexistent"),
        "stderr was: {stderr}"
    );
}
