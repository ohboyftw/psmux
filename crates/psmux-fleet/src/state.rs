//! Cold-path reader for orchestrate's `state.json`.
//!
//! Loads an `OrchestrationState` and projects each `(id, WorkerState)` entry
//! into FleetView's `Workspace` shape. Fields that require backend-pipe or
//! plan.json data (branch, worktree_path, agent_kind, task, metadata) default
//! here and are filled by warm hydration in C2.

use crate::{AgentKind, FleetError, Workspace, WorkspaceStatus};
use chrono::{DateTime, Utc};
use psmux::orchestrate::{OrchestrationState, WorkerState, WorkerStatus};
use std::path::{Path, PathBuf};

/// Reads state.json and projects orchestrate's `WorkerState` into FleetView's
/// `Workspace` model.
pub struct StateReader {
    state: OrchestrationState,
}

impl StateReader {
    /// Load and parse `state.json` from disk.
    pub fn load(path: &Path) -> Result<Self, FleetError> {
        let bytes = std::fs::read(path)?;
        let state: OrchestrationState = serde_json::from_slice(&bytes)?;
        Ok(Self { state })
    }

    /// Monotonic CAS version of the loaded state.
    pub fn state_version(&self) -> u64 {
        self.state.state_version
    }

    /// psmux session name recorded in the state file.
    pub fn session(&self) -> &str {
        &self.state.session
    }

    /// Project every worker entry into a `Workspace`.
    pub fn workspaces(&self) -> Vec<Workspace> {
        self.state
            .workers
            .iter()
            .map(|(id, ws)| worker_to_workspace(id, ws, &self.state.session))
            .collect()
    }
}

fn worker_to_workspace(id: &str, ws: &WorkerState, session: &str) -> Workspace {
    Workspace {
        id: id.to_string(),
        branch: None,
        worktree_path: None,
        pane_id: ws.pane_id.clone(),
        session: session.to_string(),
        agent_kind: AgentKind::Unknown,
        status: map_status(&ws.status),
        task: None,
        started_at: ws.started_at.as_deref().and_then(parse_ts),
        finished_at: ws.finished_at.as_deref().and_then(parse_ts),
        exit_code: ws.exit_code,
        crash_dump_path: ws.crash_dump_path.as_ref().map(PathBuf::from),
        metadata: Default::default(),
    }
}

fn map_status(s: &WorkerStatus) -> WorkspaceStatus {
    match s {
        WorkerStatus::Pending => WorkspaceStatus::Pending,
        WorkerStatus::Running => WorkspaceStatus::Running,
        WorkerStatus::Succeeded => WorkspaceStatus::Succeeded,
        WorkerStatus::Failed => WorkspaceStatus::Failed,
        WorkerStatus::Skipped => WorkspaceStatus::Skipped,
    }
}

fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}
