//! Cold-path reader for orchestrate's `state.json`.
//!
//! Loads an `OrchestrationState` and projects each `(id, WorkerState)` entry
//! into FleetView's `Workspace` shape. Fields that require backend-pipe or
//! plan.json data (branch, worktree_path, agent_kind, task, metadata) default
//! here and are filled by warm hydration in C2.

use crate::{AgentKind, FleetError, Workspace, WorkspaceStatus};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use psmux::orchestrate::{OrchestrationState, WorkerState, WorkerStatus};
use std::path::{Path, PathBuf};

/// RAII guard that releases the advisory file lock on drop. Private to this
/// module; used by [`try_cas_commit`] to guarantee the lock is released on
/// every path (happy, early-return mismatch, `?`-propagated error, panic).
struct UnlockGuard<'a>(&'a std::fs::File);
impl Drop for UnlockGuard<'_> {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

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

/// Compare-and-swap update helper (spec §6.1).
///
/// Reads `path`, applies `mutate`, attempts an atomic write iff the on-disk
/// `state_version` still matches the version observed at the start. Retries
/// up to 3 times on mismatch. Returns [`FleetError::StateVersionMismatch`]
/// if all retries collide.
///
/// # Logging
///
/// Every retry prints a line to stderr in the form
/// `fleet_state_cas_retries_total attempt=N path=P`
/// so operators can tune the retry budget from real traffic. v1 is
/// log-scraping only; v2 will wire this into a metrics exporter.
///
/// # Closure semantics
///
/// The `mutate` closure may be invoked up to 3 times (once per attempt).
/// Each invocation receives a **freshly re-read** `OrchestrationState` from
/// disk — changes applied to `state` on a prior attempt are discarded on
/// retry. The closure MUST be idempotent: applying it N times with the same
/// logical intent must converge on the same mutation. For example,
/// `workers.insert(id, state)` is safe; `list.push(item)` is NOT — it would
/// double-append on retry.
///
/// # Concurrency model
///
/// The CAS commit step (re-read + rename) is guarded by an OS-level
/// advisory exclusive lock on the state file (via `fs2`). This serializes
/// concurrent FleetView writers. It does NOT coordinate with orchestrate's
/// existing `OrchestrationState::save()` path — orchestrate does not take
/// this lock today. If a FleetView writer and orchestrate race on the same
/// file, the CAS's version re-read is the best-effort guard; the window is
/// tight but not zero.
pub fn update_state<F>(path: &Path, mut mutate: F) -> Result<(), FleetError>
where
    F: FnMut(&mut OrchestrationState) -> Result<(), FleetError>,
{
    const MAX_RETRIES: u32 = 3;

    for attempt in 0..MAX_RETRIES {
        let bytes = std::fs::read(path)?;
        let mut state: OrchestrationState = serde_json::from_slice(&bytes)?;
        let observed = state.state_version;

        mutate(&mut state)?;
        state.state_version = observed.wrapping_add(1);

        if try_cas_commit(path, &state, observed)? {
            return Ok(());
        }
        eprintln!(
            "fleet_state_cas_retries_total attempt={} path={}",
            attempt + 1,
            path.display()
        );
    }
    Err(FleetError::StateVersionMismatch)
}

/// Re-reads the on-disk state under an advisory lock. If the on-disk
/// `state_version` still matches `observed`, writes the new state atomically
/// (tempfile + rename) and returns `true`. If not, returns `false` and the
/// caller retries.
///
/// Locking uses a sibling lock file (`<path>.lock`) rather than the state
/// file itself. On Windows `fs2::lock_exclusive` is mandatory locking, which
/// would block our own tempfile rename over the target path. A sibling lock
/// sidesteps that while still serializing concurrent FleetView writers.
fn try_cas_commit(
    path: &Path,
    state: &OrchestrationState,
    observed: u64,
) -> Result<bool, FleetError> {
    let parent = path.parent().ok_or(FleetError::BackendDown)?;
    let lock_path = {
        let mut p = path.as_os_str().to_os_string();
        p.push(".lock");
        std::path::PathBuf::from(p)
    };

    // Acquire exclusive advisory lock on a sibling file (created if absent).
    let lock_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    lock_file.lock_exclusive().map_err(FleetError::Io)?;
    let _guard = UnlockGuard(&lock_file);

    // Re-read under the lock.
    let bytes = std::fs::read(path)?;
    let on_disk: OrchestrationState = serde_json::from_slice(&bytes)?;
    if on_disk.state_version != observed {
        return Ok(false);
    }

    // Atomic write via tempfile + rename.
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    let serialized = serde_json::to_vec_pretty(state)?;
    std::io::Write::write_all(tmp.as_file_mut(), &serialized)?;
    tmp.as_file_mut().sync_all()?;
    tmp.persist(path).map_err(|e| FleetError::Io(e.error))?;
    Ok(true)
}
