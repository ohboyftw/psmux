use psmux_fleet::state::{update_state, OrchestrationState, WorkerState};
use psmux_fleet::FleetError;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn unique_dir() -> PathBuf {
    let base = std::env::temp_dir().join(format!("fleetview-cas-{}", std::process::id()));
    let dir = base.join(rand_suffix());
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn rand_suffix() -> String {
    use rand::Rng;
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let r: u64 = rand::thread_rng().gen();
    format!("{nanos:x}-{r:x}")
}

fn seed(dir: &std::path::Path, version: u64) -> PathBuf {
    let path = dir.join("state.json");
    let state = OrchestrationState {
        session: "demo".into(),
        workers: HashMap::new(),
        state_version: version,
    };
    fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    path
}

#[test]
fn update_state_increments_version_on_clean_write() {
    let dir = unique_dir();
    let path = seed(&dir, 5);
    update_state(&path, |s| {
        s.workers.insert("w1".into(), WorkerState::new_pending());
        Ok(())
    })
    .unwrap();
    let back: OrchestrationState =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(back.state_version, 6);
    assert!(back.workers.contains_key("w1"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn update_state_retries_on_concurrent_mutation() {
    let dir = unique_dir();
    let path = seed(&dir, 0);
    let path_inner = path.clone();
    let mut call_count = 0;
    update_state(&path, |s| {
        call_count += 1;
        if call_count == 1 {
            // Simulate a concurrent writer racing between load and CAS:
            let external = OrchestrationState {
                session: s.session.clone(),
                workers: s.workers.clone(),
                state_version: s.state_version + 10,
            };
            fs::write(&path_inner, serde_json::to_string(&external).unwrap()).unwrap();
        }
        s.workers.insert("w1".into(), WorkerState::new_pending());
        Ok(())
    })
    .unwrap();
    assert!(call_count >= 2, "must have retried");
    let back: OrchestrationState =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(back.workers.contains_key("w1"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn update_state_surfaces_after_retry_exhaustion() {
    let dir = unique_dir();
    let path = seed(&dir, 0);
    let path_inner = path.clone();
    // Always force a concurrent write. Should exhaust retries and return
    // STATE_VERSION_MISMATCH.
    let res = update_state(&path, |s| {
        let external = OrchestrationState {
            session: s.session.clone(),
            workers: s.workers.clone(),
            state_version: s.state_version + 100,
        };
        fs::write(&path_inner, serde_json::to_string(&external).unwrap()).unwrap();
        Ok(())
    });
    assert!(matches!(res, Err(FleetError::StateVersionMismatch)));
    fs::remove_dir_all(&dir).ok();
}
