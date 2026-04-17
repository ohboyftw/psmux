//! DAG-driven pane orchestration from a `plan.json` file.
//!
//! Reads a plan specifying workers (id, command, dependencies), validates the
//! dependency graph is acyclic, provisions git worktrees if requested, and
//! launches panes in topological order using `wait-for --exit` for dependency
//! resolution.

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;

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
