use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum ConflictKind {
    TextConflict,
    BaseMoved,
    MergeTreeUnavailable,
}

#[derive(Debug, Error)]
pub enum FleetError {
    #[error("workspace not found: {0}")]
    WorkspaceNotFound(String),
    #[error("no psmux session exists")]
    SessionNotFound,
    #[error("multiple sessions; specify one: {0:?}")]
    SessionAmbiguous(Vec<String>),
    #[error("pane {0} is dead")]
    PaneDead(String),
    #[error("pane for workspace {0} not yet assigned")]
    PaneNotReady(String),
    #[error("workspace {0} already terminal")]
    AlreadyTerminal(String),
    #[error("backend pipe unreachable")]
    BackendDown,
    #[error("worktree {workspace} has uncommitted changes: {paths:?}")]
    WorktreeDirty {
        workspace: String,
        paths: Vec<String>,
    },
    #[error("merge conflict ({kind:?})")]
    MergeConflict {
        kind: ConflictKind,
        conflicts: Vec<String>,
        stderr: String,
    },
    #[error("another merge in progress for base {0}")]
    MergeInProgress(String),
    #[error("state.json version mismatch — CAS retries exhausted")]
    StateVersionMismatch,
    #[error("git not available on PATH")]
    GitUnavailable,
    #[error("failed to spawn orchestrate: {0}")]
    OrchestrateSpawnFailed(String),
    #[error("concurrent workspace quota exceeded: {current}/{limit}")]
    QuotaExceeded { limit: u32, current: u32 },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl FleetError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::WorkspaceNotFound(_) => "WORKSPACE_NOT_FOUND",
            Self::SessionNotFound => "SESSION_NOT_FOUND",
            Self::SessionAmbiguous(_) => "SESSION_AMBIGUOUS",
            Self::PaneDead(_) => "PANE_DEAD",
            Self::PaneNotReady(_) => "PANE_NOT_READY",
            Self::AlreadyTerminal(_) => "ALREADY_TERMINAL",
            Self::BackendDown => "BACKEND_DOWN",
            Self::WorktreeDirty { .. } => "WORKTREE_DIRTY",
            Self::MergeConflict { .. } => "MERGE_CONFLICT",
            Self::MergeInProgress(_) => "MERGE_IN_PROGRESS",
            Self::StateVersionMismatch => "STATE_VERSION_MISMATCH",
            Self::GitUnavailable => "GIT_UNAVAILABLE",
            Self::OrchestrateSpawnFailed(_) => "ORCHESTRATE_SPAWN_FAILED",
            Self::QuotaExceeded { .. } => "QUOTA_EXCEEDED",
            Self::Io(_) | Self::Json(_) => "INTERNAL",
        }
    }
}
