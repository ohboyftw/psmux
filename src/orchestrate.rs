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
    /// Path to a psmux crash report (from `crash::crash_directory()`) recorded
    /// when the worker's pane vanished without a clean exit code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crash_dump_path: Option<String>,
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
            crash_dump_path: None,
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

// ─── Worktree provisioning ──────────────────────────────────────────────────

/// Resolve a worker's `cwd` against the plan directory when relative.
fn resolve_cwd(plan_dir: &Path, cwd: &Path) -> PathBuf {
    if cwd.is_absolute() {
        cwd.to_path_buf()
    } else {
        plan_dir.join(cwd)
    }
}

/// Resolve a worktree's `repo` path against the plan directory when relative.
fn resolve_repo(plan_dir: &Path, repo: &Path) -> PathBuf {
    if repo.is_absolute() {
        repo.to_path_buf()
    } else {
        plan_dir.join(repo)
    }
}

/// Build the `git worktree add` args for a worker. Separated for testing.
fn worktree_add_args(cwd: &Path, spec: &WorktreeSpec) -> Vec<String> {
    let mut args = vec![
        "worktree".to_string(),
        "add".to_string(),
        cwd.to_string_lossy().into_owned(),
        "-b".to_string(),
        spec.branch.clone(),
    ];
    if let Some(base) = &spec.base {
        args.push(base.clone());
    }
    args
}

/// For each worker with a `worktree`, run `git worktree add` inside the
/// worker's resolved repo directory.
pub fn provision_worktrees(plan: &Plan, plan_dir: &Path) -> Result<(), String> {
    for w in &plan.workers {
        let Some(spec) = &w.worktree else {
            continue;
        };
        let cwd = match &w.cwd {
            Some(c) => resolve_cwd(plan_dir, c),
            None => {
                return Err(format!("worker {} has worktree but no cwd", w.id));
            }
        };
        let repo = resolve_repo(plan_dir, &spec.repo);
        let args = worktree_add_args(&cwd, spec);
        let output = std::process::Command::new("git")
            .current_dir(&repo)
            .args(&args)
            .output()
            .map_err(|e| format!("worker {}: failed to spawn git: {e}", w.id))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "worker {}: git worktree add failed: {}",
                w.id,
                stderr.trim()
            ));
        }
    }
    Ok(())
}

/// For each worker with a `worktree`, run `git worktree remove`. Paths are
/// resolved against `plan_dir` exactly as in [`provision_worktrees`] so
/// `--cleanup` finds the worktrees regardless of the directory orchestrate
/// was invoked from (symmetric to the plan_dir resolution on the add path).
pub fn cleanup_worktrees(plan: &Plan, plan_dir: &Path) -> Result<(), String> {
    for w in &plan.workers {
        let Some(spec) = &w.worktree else { continue };
        let Some(cwd) = &w.cwd else { continue };
        let cwd = resolve_cwd(plan_dir, cwd);
        let repo = resolve_repo(plan_dir, &spec.repo);
        let output = std::process::Command::new("git")
            .current_dir(&repo)
            .args(["worktree", "remove", &cwd.to_string_lossy()])
            .output()
            .map_err(|e| format!("worker {}: failed to spawn git: {e}", w.id))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "worker {}: git worktree remove failed: {}",
                w.id,
                stderr.trim()
            ));
        }
    }
    Ok(())
}

// ─── Dependents / failure propagation ───────────────────────────────────────

/// Transitive closure of workers that directly or indirectly depend on
/// `worker_id`. Does not include `worker_id` itself.
pub fn dependents(plan: &Plan, worker_id: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut frontier: Vec<String> = vec![worker_id.to_string()];
    while let Some(current) = frontier.pop() {
        for w in &plan.workers {
            if w.depends_on.iter().any(|d| d == &current) && out.insert(w.id.clone()) {
                frontier.push(w.id.clone());
            }
        }
    }
    out
}

/// Mark all transitive dependents of `failed_id` as `Skipped` (unless already
/// terminal).
fn skip_dependents(plan: &Plan, state: &mut OrchestrationState, failed_id: &str) {
    for dep_id in dependents(plan, failed_id) {
        let entry = state
            .workers
            .entry(dep_id)
            .or_insert_with(WorkerState::new_pending);
        if !entry.is_terminal() {
            entry.status = WorkerStatus::Skipped;
            entry.finished_at = Some(now_iso8601());
        }
    }
}

// ─── Orchestration loop ─────────────────────────────────────────────────────

/// ISO-8601 timestamp helper.
fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Scan recent psmux crash reports for one that likely belongs to this worker.
/// Prefers a PID match; falls back to the newest crash after `started_at`.
fn find_crash_for(ws: &WorkerState) -> Option<String> {
    let entries = crate::crash::list_crashes(10);
    if entries.is_empty() {
        return None;
    }
    if let Some(pid) = ws.pid {
        if let Some(e) = entries.iter().find(|e| e.pid == pid) {
            return Some(e.path.to_string_lossy().into_owned());
        }
    }
    let started_ts = ws
        .started_at
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp() as u64);
    let candidate = entries.first()?;
    match started_ts {
        Some(start) if candidate.timestamp >= start => {
            Some(candidate.path.to_string_lossy().into_owned())
        }
        _ => None,
    }
}

/// Path to the psmux binary currently executing this orchestrator.
fn psmux_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("cannot resolve psmux binary: {e}"))
}

/// Enable or disable remain-on-exit for an orchestrate session so dead panes
/// stay visible long enough for the poll loop to read their exit codes.
fn set_remain_on_exit(exe: &Path, session: &str, on: bool) {
    let value = if on { "on" } else { "off" };
    // Best-effort: ignore errors — the session may be gone by the time we try
    // to restore the option, and a spurious failure here must not abort the run.
    let _ = std::process::Command::new(exe)
        .args(["set-option", "-t", session, "remain-on-exit", value])
        .output();
}

/// Kill a preserved dead pane so it doesn't accumulate in the session.
fn kill_worker_pane(exe: &Path, pane_id: &str) {
    // Best-effort: pane may already be gone; ignore the result.
    let _ = std::process::Command::new(exe)
        .args(["kill-pane", "-t", pane_id])
        .output();
}

/// Spawn a pane for `worker` using `psmux new-window -P`. Returns pane id.
fn spawn_worker_pane(plan: &Plan, plan_dir: &Path, worker: &Worker) -> Result<String, String> {
    let exe = psmux_exe()?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.args([
        "new-window",
        "--raw",
        "-t",
        &plan.session,
        "-n",
        &worker.id,
        "-P",
        "-F",
        "#{pane_id}",
    ]);
    if let Some(cwd) = &worker.cwd {
        cmd.args(["-c", &resolve_cwd(plan_dir, cwd).to_string_lossy()]);
    }
    if let Some(env) = &worker.env {
        for (k, v) in env {
            cmd.env(k, v);
        }
    }
    cmd.arg("--");
    for part in &worker.command {
        cmd.arg(part);
    }

    let output = cmd
        .output()
        .map_err(|e| format!("worker {}: failed to spawn psmux: {e}", worker.id))?;
    if !output.status.success() {
        return Err(format!(
            "worker {}: psmux new-window failed: {}",
            worker.id,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let pane_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if pane_id.is_empty() {
        return Err(format!(
            "worker {}: psmux new-window returned empty pane id",
            worker.id
        ));
    }
    Ok(pane_id)
}

/// Query a pane's dead/exit-code status via `psmux list-panes`.
/// Returns `Ok(Some(exit_code))` if the pane is dead, `Ok(None)` if still alive.
///
/// Targets `list-panes -t <session>` (session-name routing is unambiguous — it
/// reads `<session>.port` directly) and then filters for the requested
/// `pane_id`. Targeting the pane id directly would go through the ambiguous
/// reverse-mtime scan in session::resolve_server_for_command and could land on
/// the wrong server if multiple live servers exist (common under test harnesses
/// when the user's attached terminal shares a numeric id).
/// Exit-code sentinels emitted by [`check_pane_exit`] when the real worker
/// exit code isn't available. Distinguishing these from genuine worker exits
/// matters for downstream CI: a killed session is not the same as a crash.
pub const EXIT_PANE_GONE: i32 = -1;
pub const EXIT_SESSION_GONE: i32 = -3;

fn check_pane_exit(session: &str, pane_id: &str) -> Result<Option<i32>, String> {
    let exe = psmux_exe()?;
    let output = std::process::Command::new(&exe)
        .args([
            "list-panes",
            "-t",
            session,
            "-s",
            "-F",
            "#{pane_id}:#{pane_dead}:#{pane_exit_code}",
        ])
        .output()
        .map_err(|e| format!("list-panes failed to spawn: {e}"))?;
    if !output.status.success() {
        // The whole session is gone (user killed it, or server died).
        // Distinct from per-pane pruning so callers can tell the two apart.
        return Ok(Some(EXIT_SESSION_GONE));
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split(':');
        let reported_id = parts.next().unwrap_or("").trim();
        if reported_id != pane_id {
            continue;
        }
        let dead_flag = parts.next().unwrap_or("0").trim();
        let exit_raw = parts.next().unwrap_or("").trim();
        let dead = matches!(dead_flag, "1" | "true");
        if !dead {
            return Ok(None);
        }
        return Ok(Some(exit_raw.parse::<i32>().unwrap_or(EXIT_PANE_GONE)));
    }
    // Pane not found in any window of the session — pruned by the server.
    Ok(Some(EXIT_PANE_GONE))
}

/// Main orchestration loop: spawn ready workers, poll for completion, advance
/// dependent workers as predecessors succeed. Returns `Ok(())` once every
/// worker has reached a terminal state.
pub fn run_plan(
    plan: &Plan,
    state: &mut OrchestrationState,
    plan_dir: &Path,
    state_path: &Path,
    timeout_ms: Option<u64>,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    // Enable remain-on-exit for this session before any worker pane is spawned.
    // This ensures that even a very short-lived worker (e.g. `cmd /c echo`)
    // that exits before the next 500 ms poll tick is preserved as a dead pane
    // whose exit code remains observable via #{pane_dead}/#{pane_exit_code}.
    // Dead panes are explicitly killed after the poll loop reads their codes.
    if let Ok(exe) = psmux_exe() {
        set_remain_on_exit(&exe, &plan.session, true);
    }
    loop {
        let all_terminal = plan.workers.iter().all(|w| {
            state
                .workers
                .get(&w.id)
                .is_some_and(WorkerState::is_terminal)
        });
        if all_terminal {
            return Ok(());
        }
        // Hard timeout: mark still-running workers as Failed with exit_code=-2
        // ("timed out") and return. Gives CI/test harnesses a ceiling even when
        // a worker hangs indefinitely.
        if let Some(limit) = timeout_ms {
            if started.elapsed().as_millis() as u64 >= limit {
                for ws in state.workers.values_mut() {
                    if ws.status == WorkerStatus::Running || ws.status == WorkerStatus::Pending {
                        ws.status = WorkerStatus::Failed;
                        ws.exit_code = Some(-2);
                        ws.finished_at = Some(now_iso8601());
                    }
                }
                state
                    .save(state_path)
                    .map_err(|e| format!("save state: {e}"))?;
                return Err(format!("orchestrate timed out after {} ms", limit));
            }
        }

        // Launch any ready, still-pending workers.
        let completed = state.completed_ids();
        let ready: Vec<&Worker> = plan.ready_workers(&completed);
        for worker in ready {
            let ws = state
                .workers
                .get(&worker.id)
                .cloned()
                .unwrap_or_else(WorkerState::new_pending);
            if ws.status != WorkerStatus::Pending {
                continue;
            }
            match spawn_worker_pane(plan, plan_dir, worker) {
                Ok(pane_id) => {
                    let mut ws = ws;
                    ws.status = WorkerStatus::Running;
                    ws.pane_id = Some(pane_id);
                    ws.started_at = Some(now_iso8601());
                    state.workers.insert(worker.id.clone(), ws);
                    state
                        .save(state_path)
                        .map_err(|e| format!("save state: {e}"))?;
                }
                Err(e) => {
                    let mut ws = ws;
                    ws.status = WorkerStatus::Failed;
                    ws.exit_code = Some(-1);
                    ws.finished_at = Some(now_iso8601());
                    state.workers.insert(worker.id.clone(), ws);
                    skip_dependents(plan, state, &worker.id);
                    state
                        .save(state_path)
                        .map_err(|se| format!("save state: {se}"))?;
                    eprintln!("psmux orchestrate: {e}");
                }
            }
        }

        // Poll running panes for exit.
        let running: Vec<(String, String)> = state
            .workers
            .iter()
            .filter(|(_, ws)| ws.status == WorkerStatus::Running)
            .filter_map(|(id, ws)| ws.pane_id.as_ref().map(|p| (id.clone(), p.clone())))
            .collect();

        let mut updated = false;
        for (worker_id, pane_id) in running {
            let exit = check_pane_exit(&plan.session, &pane_id)?;
            let Some(code) = exit else { continue };
            if let Some(ws) = state.workers.get_mut(&worker_id) {
                ws.exit_code = Some(code);
                ws.finished_at = Some(now_iso8601());
                ws.status = if code == 0 {
                    WorkerStatus::Succeeded
                } else {
                    WorkerStatus::Failed
                };
                // Pane vanished with no usable exit code — try to associate a
                // psmux crash report for post-mortem inspection. Only do this
                // for EXIT_PANE_GONE (worker disappeared from an otherwise live
                // session); EXIT_SESSION_GONE means the whole session was torn
                // down and there's no crash to attribute.
                if code == EXIT_PANE_GONE {
                    if let Some(path) = find_crash_for(ws) {
                        ws.crash_dump_path = Some(path);
                    }
                }
            }
            if state
                .workers
                .get(&worker_id)
                .is_some_and(|ws| ws.status == WorkerStatus::Failed)
            {
                skip_dependents(plan, state, &worker_id);
            }
            updated = true;
            // The pane exited with a real exit code (remain-on-exit kept it
            // visible as a dead pane). Kill it now so dead panes don't
            // accumulate in the session. EXIT_PANE_GONE means the pane was
            // already reaped, so no kill is needed in that case.
            if code != EXIT_PANE_GONE && code != EXIT_SESSION_GONE {
                if let Ok(exe) = psmux_exe() {
                    kill_worker_pane(&exe, &pane_id);
                }
            }
        }

        if updated {
            state
                .save(state_path)
                .map_err(|e| format!("save state: {e}"))?;
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
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
        assert!(
            !tmp.exists(),
            "tmp file must be renamed away, not left behind"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn worktree_add_args_without_base() {
        let spec = WorktreeSpec {
            repo: PathBuf::from("."),
            branch: "feat-x".into(),
            base: None,
        };
        let args = worktree_add_args(Path::new("/tmp/wt-x"), &spec);
        assert_eq!(args, vec!["worktree", "add", "/tmp/wt-x", "-b", "feat-x"]);
    }

    #[test]
    fn worktree_add_args_with_base() {
        let spec = WorktreeSpec {
            repo: PathBuf::from("."),
            branch: "feat-y".into(),
            base: Some("origin/main".into()),
        };
        let args = worktree_add_args(Path::new("/tmp/wt-y"), &spec);
        assert_eq!(
            args,
            vec![
                "worktree",
                "add",
                "/tmp/wt-y",
                "-b",
                "feat-y",
                "origin/main"
            ]
        );
    }

    #[test]
    fn dependents_returns_transitive_closure() {
        let plan = plan_from(
            r#"{
                "version": 1,
                "session": "s",
                "workers": [
                    { "id": "a", "command": ["x"] },
                    { "id": "b", "command": ["y"], "depends_on": ["a"] },
                    { "id": "c", "command": ["z"], "depends_on": ["b"] },
                    { "id": "d", "command": ["w"] }
                ]
            }"#,
        );
        let deps = dependents(&plan, "a");
        assert!(deps.contains("b"));
        assert!(deps.contains("c"));
        assert!(!deps.contains("d"));
        assert!(!deps.contains("a"));
    }

    #[test]
    fn skip_dependents_marks_downstream_skipped() {
        let plan = plan_from(
            r#"{
                "version": 1,
                "session": "s",
                "workers": [
                    { "id": "a", "command": ["x"] },
                    { "id": "b", "command": ["y"], "depends_on": ["a"] },
                    { "id": "c", "command": ["z"], "depends_on": ["b"] },
                    { "id": "d", "command": ["w"] }
                ]
            }"#,
        );
        let mut state = OrchestrationState::new_for_plan(&plan);
        super::skip_dependents(&plan, &mut state, "a");
        assert_eq!(state.workers["b"].status, WorkerStatus::Skipped);
        assert_eq!(state.workers["c"].status, WorkerStatus::Skipped);
        assert_eq!(state.workers["d"].status, WorkerStatus::Pending);
    }

    #[test]
    fn skip_dependents_preserves_already_succeeded() {
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
        state.workers.get_mut("b").unwrap().status = WorkerStatus::Succeeded;
        super::skip_dependents(&plan, &mut state, "a");
        assert_eq!(state.workers["b"].status, WorkerStatus::Succeeded);
    }
}
