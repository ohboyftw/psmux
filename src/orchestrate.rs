//! DAG-driven pane orchestration from a `plan.json` file.
//!
//! Reads a plan specifying workers (id, command, dependencies), validates the
//! dependency graph is acyclic, provisions git worktrees if requested, and
//! launches panes in topological order using `wait-for --exit` for dependency
//! resolution.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A parsed orchestration plan.
#[derive(Debug, Deserialize)]
pub struct Plan {
    /// Must be 1.
    pub version: u32,
    /// Session name to create or use.
    pub session: String,
    /// Worker definitions.
    pub workers: Vec<Worker>,
}

/// A single worker in the plan.
#[derive(Debug, Deserialize)]
pub struct Worker {
    /// Unique identifier for this worker.
    pub id: String,
    /// Working directory (absolute or relative to plan file).
    pub cwd: Option<PathBuf>,
    /// Command to run in the pane.
    pub command: Vec<String>,
    /// IDs of workers that must complete before this one starts.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// If present, create a git worktree at `cwd` before launching.
    pub worktree: Option<WorktreeSpec>,
    /// Extra environment variables for this worker's pane.
    pub env: Option<HashMap<String, String>>,
}

/// Specification for creating a git worktree.
#[derive(Debug, Deserialize)]
pub struct WorktreeSpec {
    /// Path to the repository (can be relative to plan file).
    pub repo: PathBuf,
    /// Branch name to create.
    pub branch: String,
    /// Base ref to branch from (defaults to HEAD).
    pub base: Option<String>,
}

/// Errors during plan validation.
#[derive(Debug)]
pub enum PlanError {
    /// Plan version is not supported.
    UnsupportedVersion(u32),
    /// No workers in the plan.
    EmptyPlan,
    /// Two workers share the same id.
    DuplicateId(String),
    /// A depends_on references a worker id that doesn't exist.
    UnknownDependency { worker: String, dependency: String },
    /// The dependency graph contains a cycle.
    CycleDetected,
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(v) => write!(f, "unsupported plan version: {v} (expected 1)"),
            Self::EmptyPlan => write!(f, "plan has no workers"),
            Self::DuplicateId(id) => write!(f, "duplicate worker id: {id}"),
            Self::UnknownDependency { worker, dependency } => {
                write!(f, "worker {worker} depends on unknown id: {dependency}")
            }
            Self::CycleDetected => write!(f, "dependency cycle detected"),
        }
    }
}

impl std::error::Error for PlanError {}

impl Plan {
    /// Validate the plan: check version, uniqueness, and DAG acyclicity.
    pub fn validate(&self) -> Result<(), PlanError> {
        if self.version != 1 {
            return Err(PlanError::UnsupportedVersion(self.version));
        }
        if self.workers.is_empty() {
            return Err(PlanError::EmptyPlan);
        }

        // Check for duplicate IDs
        let mut seen = HashSet::new();
        for w in &self.workers {
            if !seen.insert(&w.id) {
                return Err(PlanError::DuplicateId(w.id.clone()));
            }
        }

        // Check all dependencies reference existing workers
        let ids: HashSet<&str> = self.workers.iter().map(|w| w.id.as_str()).collect();
        for w in &self.workers {
            for dep in &w.depends_on {
                if !ids.contains(dep.as_str()) {
                    return Err(PlanError::UnknownDependency {
                        worker: w.id.clone(),
                        dependency: dep.clone(),
                    });
                }
            }
        }

        // Topological sort to detect cycles (Kahn's algorithm)
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for w in &self.workers {
            in_degree.entry(w.id.as_str()).or_insert(0);
            adj.entry(w.id.as_str()).or_default();
            for dep in &w.depends_on {
                adj.entry(dep.as_str()).or_default().push(w.id.as_str());
                *in_degree.entry(w.id.as_str()).or_insert(0) += 1;
            }
        }

        let mut queue: Vec<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();
        let mut visited = 0;

        while let Some(node) = queue.pop() {
            visited += 1;
            if let Some(neighbors) = adj.get(node) {
                for &next in neighbors {
                    let deg = in_degree.get_mut(next).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(next);
                    }
                }
            }
        }

        if visited != self.workers.len() {
            return Err(PlanError::CycleDetected);
        }

        Ok(())
    }

    /// Return workers whose dependencies are all in `completed`.
    pub fn ready_workers(&self, completed: &HashSet<String>) -> Vec<&Worker> {
        self.workers
            .iter()
            .filter(|w| {
                !completed.contains(&w.id) && w.depends_on.iter().all(|dep| completed.contains(dep))
            })
            .collect()
    }
}

// ─── State store ────────────────────────────────────────────────────────────

/// Persistent orchestration state for a single plan run.
#[derive(Debug, Serialize, Deserialize)]
pub struct OrchestrationState {
    pub session: String,
    pub workers: HashMap<String, WorkerState>,
}

/// Per-worker runtime state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerState {
    pub status: WorkerStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
}

impl WorkerState {
    pub fn new_pending() -> Self {
        Self {
            status: WorkerStatus::Pending,
            pane_id: None,
            pid: None,
            exit_code: None,
            started_at: None,
            finished_at: None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            WorkerStatus::Succeeded | WorkerStatus::Failed | WorkerStatus::Skipped
        )
    }
}

impl OrchestrationState {
    /// Initialize a fresh state with all workers `Pending`.
    pub fn new_for_plan(plan: &Plan) -> Self {
        let workers = plan
            .workers
            .iter()
            .map(|w| (w.id.clone(), WorkerState::new_pending()))
            .collect();
        Self {
            session: plan.session.clone(),
            workers,
        }
    }

    /// Atomically write state to `path` via a sibling `.tmp` file + rename.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut tmp = path.to_path_buf();
        let tmp_name = match path.file_name() {
            Some(n) => {
                let mut s = n.to_os_string();
                s.push(".tmp");
                s
            }
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "state path has no file name",
                ));
            }
        };
        tmp.set_file_name(tmp_name);

        let data = serde_json::to_vec_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(&tmp, &data)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Read and deserialize state from `path`.
    pub fn load(path: &Path) -> io::Result<Self> {
        let data = fs::read(path)?;
        serde_json::from_slice(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Set of worker ids whose status is terminal (succeeded/failed/skipped).
    pub fn completed_ids(&self) -> HashSet<String> {
        self.workers
            .iter()
            .filter(|(_, ws)| ws.is_terminal())
            .map(|(id, _)| id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_from(json: &str) -> Plan {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn state_roundtrips_through_save_load() {
        let plan = plan_from(
            r#"{
                "version": 1,
                "session": "s",
                "workers": [
                    { "id": "a", "command": ["x"] },
                    { "id": "b", "command": ["y"], "depends_on": ["a"] }
                ]
            }"#,
        );
        let mut state = OrchestrationState::new_for_plan(&plan);
        state.workers.get_mut("a").unwrap().status = WorkerStatus::Succeeded;
        state.workers.get_mut("a").unwrap().exit_code = Some(0);
        state.workers.get_mut("a").unwrap().pid = Some(12345);

        let tmp = std::env::temp_dir().join(format!(
            "psmux_orch_state_{}_{}.json",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        state.save(&tmp).unwrap();

        let loaded = OrchestrationState::load(&tmp).unwrap();
        assert_eq!(loaded.session, "s");
        assert_eq!(
            loaded.workers.get("a").unwrap().status,
            WorkerStatus::Succeeded
        );
        assert_eq!(loaded.workers.get("a").unwrap().pid, Some(12345));
        assert_eq!(
            loaded.workers.get("b").unwrap().status,
            WorkerStatus::Pending
        );

        fs::remove_file(&tmp).ok();
    }

    #[test]
    fn atomic_save_removes_tmp_sibling_after_success() {
        let plan = plan_from(
            r#"{
                "version": 1,
                "session": "s",
                "workers": [{ "id": "a", "command": ["x"] }]
            }"#,
        );
        let state = OrchestrationState::new_for_plan(&plan);
        let dir = std::env::temp_dir().join(format!("psmux_orch_atomic_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        state.save(&path).unwrap();

        assert!(path.exists(), "state.json should exist");
        let tmp = dir.join("state.json.tmp");
        assert!(!tmp.exists(), "tmp file must be renamed away, not left behind");

        fs::remove_dir_all(&dir).ok();
    }
}
