//! Integration test for `psmux fleet ls --json`.
//! Seeds a fake state.json, invokes the compiled binary, parses JSON output.

use std::fs;
use std::process::Command;

fn psmux_bin() -> &'static str {
    env!("CARGO_BIN_EXE_psmux")
}

fn unique_dir() -> std::path::PathBuf {
    let base = std::env::temp_dir().join(format!("fleetview-cli-ls-{}", std::process::id()));
    let dir = base.join(rand_suffix());
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{nanos:x}")
}

#[test]
fn fleet_ls_json_reads_state_and_emits_workspaces() {
    let dir = unique_dir();
    let state_path = dir.join("state.json");
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
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("parse JSON output");
    let workspaces = parsed
        .get("workspaces")
        .and_then(|v| v.as_array())
        .expect("workspaces array");
    assert_eq!(workspaces.len(), 3);
    assert_eq!(parsed.get("total").and_then(|v| v.as_u64()), Some(3));

    let _ = fs::remove_dir_all(&dir);
}
