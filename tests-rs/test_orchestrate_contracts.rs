//! Contract tests for `orchestrate` module (Phase 2, Feature 6).
//!
//! Validates plan.json parsing, DAG validation, and state store contracts.

use psmux::orchestrate::{Plan, PlanError};

// ─── Schema parsing ─────────────────────────────────────────────────────────

#[test]
fn parse_minimal_plan() {
    let json = r#"{
        "version": 1,
        "session": "test-session",
        "workers": [
            { "id": "a", "command": ["echo", "hello"] }
        ]
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    assert_eq!(plan.version, 1);
    assert_eq!(plan.session, "test-session");
    assert_eq!(plan.workers.len(), 1);
    assert_eq!(plan.workers[0].id, "a");
    assert_eq!(plan.workers[0].command, vec!["echo", "hello"]);
}

#[test]
fn parse_plan_with_dependencies() {
    let json = r#"{
        "version": 1,
        "session": "dep-test",
        "workers": [
            { "id": "a", "command": ["build"] },
            { "id": "b", "command": ["test"], "depends_on": ["a"] },
            { "id": "c", "command": ["deploy"], "depends_on": ["a", "b"] }
        ]
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    assert_eq!(plan.workers.len(), 3);
    assert!(plan.workers[1].depends_on.contains(&"a".to_string()));
    assert_eq!(plan.workers[2].depends_on.len(), 2);
}

#[test]
fn parse_plan_with_worktree() {
    let json = r#"{
        "version": 1,
        "session": "wt-test",
        "workers": [
            {
                "id": "feat",
                "command": ["cargo", "test"],
                "worktree": { "repo": ".", "branch": "feat-x" }
            }
        ]
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    let wt = plan.workers[0].worktree.as_ref().unwrap();
    assert_eq!(wt.branch, "feat-x");
}

#[test]
fn parse_plan_with_env() {
    let json = r#"{
        "version": 1,
        "session": "env-test",
        "workers": [
            {
                "id": "w",
                "command": ["run"],
                "env": { "FOO": "bar", "BAZ": "1" }
            }
        ]
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    let env = plan.workers[0].env.as_ref().unwrap();
    assert_eq!(env.get("FOO").unwrap(), "bar");
}

#[test]
fn parse_plan_with_cwd() {
    let json = r#"{
        "version": 1,
        "session": "cwd-test",
        "workers": [
            { "id": "w", "command": ["ls"], "cwd": "/tmp/work" }
        ]
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    assert_eq!(
        plan.workers[0].cwd.as_ref().unwrap().to_str().unwrap(),
        "/tmp/work"
    );
}

// ─── DAG validation ─────────────────────────────────────────────────────────

#[test]
fn validate_rejects_duplicate_ids() {
    let json = r#"{
        "version": 1,
        "session": "dup",
        "workers": [
            { "id": "a", "command": ["x"] },
            { "id": "a", "command": ["y"] }
        ]
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    let err = plan.validate().unwrap_err();
    assert!(matches!(err, PlanError::DuplicateId(_)));
}

#[test]
fn validate_rejects_unknown_dependency() {
    let json = r#"{
        "version": 1,
        "session": "unknown-dep",
        "workers": [
            { "id": "a", "command": ["x"], "depends_on": ["nonexistent"] }
        ]
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    let err = plan.validate().unwrap_err();
    assert!(matches!(err, PlanError::UnknownDependency { .. }));
}

#[test]
fn validate_rejects_self_dependency() {
    let json = r#"{
        "version": 1,
        "session": "self-dep",
        "workers": [
            { "id": "a", "command": ["x"], "depends_on": ["a"] }
        ]
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    let err = plan.validate().unwrap_err();
    assert!(matches!(err, PlanError::CycleDetected));
}

#[test]
fn validate_rejects_cycle() {
    let json = r#"{
        "version": 1,
        "session": "cycle",
        "workers": [
            { "id": "a", "command": ["x"], "depends_on": ["b"] },
            { "id": "b", "command": ["y"], "depends_on": ["a"] }
        ]
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    let err = plan.validate().unwrap_err();
    assert!(matches!(err, PlanError::CycleDetected));
}

#[test]
fn validate_rejects_three_node_cycle() {
    let json = r#"{
        "version": 1,
        "session": "cycle3",
        "workers": [
            { "id": "a", "command": ["x"], "depends_on": ["c"] },
            { "id": "b", "command": ["y"], "depends_on": ["a"] },
            { "id": "c", "command": ["z"], "depends_on": ["b"] }
        ]
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    let err = plan.validate().unwrap_err();
    assert!(matches!(err, PlanError::CycleDetected));
}

#[test]
fn validate_accepts_valid_dag() {
    let json = r#"{
        "version": 1,
        "session": "valid",
        "workers": [
            { "id": "a", "command": ["build"] },
            { "id": "b", "command": ["test"], "depends_on": ["a"] },
            { "id": "c", "command": ["lint"] },
            { "id": "d", "command": ["deploy"], "depends_on": ["b", "c"] }
        ]
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    assert!(plan.validate().is_ok());
}

#[test]
fn validate_rejects_empty_workers() {
    let json = r#"{
        "version": 1,
        "session": "empty",
        "workers": []
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    let err = plan.validate().unwrap_err();
    assert!(matches!(err, PlanError::EmptyPlan));
}

#[test]
fn validate_rejects_wrong_version() {
    let json = r#"{
        "version": 99,
        "session": "bad-ver",
        "workers": [
            { "id": "a", "command": ["x"] }
        ]
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    let err = plan.validate().unwrap_err();
    assert!(matches!(err, PlanError::UnsupportedVersion(_)));
}

// ─── Topology helpers ───────────────────────────────────────────────────────

#[test]
fn ready_workers_returns_those_with_no_deps() {
    let json = r#"{
        "version": 1,
        "session": "topo",
        "workers": [
            { "id": "a", "command": ["x"] },
            { "id": "b", "command": ["y"], "depends_on": ["a"] },
            { "id": "c", "command": ["z"] }
        ]
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    let completed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let ready = plan.ready_workers(&completed);
    let ids: Vec<&str> = ready.iter().map(|w| w.id.as_str()).collect();
    assert!(ids.contains(&"a"));
    assert!(ids.contains(&"c"));
    assert!(!ids.contains(&"b"));
}

#[test]
fn ready_workers_unblocks_after_dep_completes() {
    let json = r#"{
        "version": 1,
        "session": "topo2",
        "workers": [
            { "id": "a", "command": ["x"] },
            { "id": "b", "command": ["y"], "depends_on": ["a"] }
        ]
    }"#;
    let plan: Plan = serde_json::from_str(json).unwrap();
    let mut completed = std::collections::HashSet::new();
    completed.insert("a".to_string());
    let ready = plan.ready_workers(&completed);
    let ids: Vec<&str> = ready.iter().map(|w| w.id.as_str()).collect();
    assert!(ids.contains(&"b"));
}
