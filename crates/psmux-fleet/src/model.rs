use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Which agent runtime is driving the workspace's pane.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    /// Claude Code CLI.
    Claude,
    /// OpenAI Codex CLI.
    Codex,
    /// Pi coding agent (pi.dev).
    Pi,
    /// Plain shell — no agent runtime detected.
    Shell,
    /// Default for panes without an `@agent` tag or detector hit.
    #[default]
    Unknown,
}

/// Lifecycle state of a fleet workspace.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStatus {
    /// Declared in the plan but not yet launched.
    Pending,
    /// Pane is alive and the agent is working.
    Running,
    /// Pane is alive but blocked on human input (e.g. Claude permission prompt).
    Awaiting,
    /// Pane exited with code 0.
    Succeeded,
    /// Pane exited with a non-zero code or crashed.
    Failed,
    /// Dependency failed; this workspace was never launched.
    Skipped,
}

/// Uniform view of a fleet workspace, regardless of spawn origin
/// (orchestrate worker, `fleet.new` spawn, or pre-existing `@agent`-tagged pane).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    /// Stable identifier within the fleet (e.g. worker id or slug).
    pub id: String,
    /// Git branch backing the worktree, if any.
    pub branch: Option<String>,
    /// Filesystem path to the git worktree, if any.
    pub worktree_path: Option<PathBuf>,
    /// psmux pane id (e.g. `%3`); `None` before launch or after cleanup.
    pub pane_id: Option<String>,
    /// psmux session hosting the pane.
    pub session: String,
    /// Agent runtime inferred from env vars, `@agent` tags, or detectors.
    pub agent_kind: AgentKind,
    /// Current lifecycle state.
    pub status: WorkspaceStatus,
    /// Human-readable task description, if the caller provided one.
    pub task: Option<String>,
    /// Wall-clock time the pane was launched.
    pub started_at: Option<DateTime<Utc>>,
    /// Wall-clock time the pane exited.
    pub finished_at: Option<DateTime<Utc>>,
    /// Process exit code once the pane terminates.
    pub exit_code: Option<i32>,
    /// Path to a crash dump if the pane died without a clean exit.
    pub crash_dump_path: Option<PathBuf>,
    /// Free-form key/value tags carried from the plan or detectors.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}
