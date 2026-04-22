use psmux_fleet::FleetError;

#[test]
fn error_codes_match_spec() {
    let cases = [
        (
            FleetError::WorkspaceNotFound("id".into()),
            "WORKSPACE_NOT_FOUND",
        ),
        (FleetError::SessionNotFound, "SESSION_NOT_FOUND"),
        (
            FleetError::SessionAmbiguous(vec!["a".into(), "b".into()]),
            "SESSION_AMBIGUOUS",
        ),
        (FleetError::PaneDead("%3".into()), "PANE_DEAD"),
        (FleetError::PaneNotReady("id".into()), "PANE_NOT_READY"),
        (FleetError::AlreadyTerminal("id".into()), "ALREADY_TERMINAL"),
        (FleetError::BackendDown, "BACKEND_DOWN"),
        (
            FleetError::WorktreeDirty {
                workspace: "id".into(),
                paths: vec![],
            },
            "WORKTREE_DIRTY",
        ),
        (
            FleetError::MergeConflict {
                kind: psmux_fleet::ConflictKind::TextConflict,
                conflicts: vec![],
                stderr: String::new(),
            },
            "MERGE_CONFLICT",
        ),
        (
            FleetError::MergeInProgress("base".into()),
            "MERGE_IN_PROGRESS",
        ),
        (FleetError::StateVersionMismatch, "STATE_VERSION_MISMATCH"),
        (FleetError::GitUnavailable, "GIT_UNAVAILABLE"),
        (
            FleetError::OrchestrateSpawnFailed("reason".into()),
            "ORCHESTRATE_SPAWN_FAILED",
        ),
        (
            FleetError::QuotaExceeded {
                limit: 16,
                current: 16,
            },
            "QUOTA_EXCEEDED",
        ),
    ];
    for (err, expected_code) in cases {
        assert_eq!(err.code(), expected_code);
    }
}
