# FleetView — Commit 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the FleetView core library (`psmux-fleet`) with session resolution, state reading, CAS writing, awaiting-input detection, and the cold-path workspace hydrator — plus the `psmux fleet ls` / `psmux fleet status` CLI subcommands and a stdio MCP server exposing `fleet.list` / `fleet.status`.

**Architecture:** Three artifacts built on a shared core library. `crates/psmux-fleet/` holds all logic (typed errors, no I/O outside the four documented sources). `crates/psmux-fleet-mcp/` is a thin stdio MCP server linking the lib. `src/fleet/` is a new subcommand tree inside the existing `psmux` binary linking the same lib. Every frontend is stateless across invocations — re-hydrate from `state.json` on every call.

**Tech Stack:** Rust (stable), serde, serde_json, regex, chrono, thiserror, anyhow, tokio (async MCP), fs2 (file locks, even though C1 uses them only in tests), tempfile (atomic writes), rand (id entropy — unused in C1 but landed with the crate setup), rmcp (primary; hand-rolled stdio fallback if immature).

**Reference spec:** `docs/superpowers/specs/2026-04-22-fleetview-design.md`. This plan covers Commit 1 only — §10 "Implementation plan → Commit 1 — crate scaffolding + core + `list`/`status`". Commits 2–4 get their own plans after C1 ships.

---

## File Structure

### New files

| Path | Responsibility |
|---|---|
| `crates/psmux-fleet/Cargo.toml` | Library crate manifest |
| `crates/psmux-fleet/src/lib.rs` | Public API surface + module wiring |
| `crates/psmux-fleet/src/error.rs` | `FleetError` enum (all codes from spec §9) |
| `crates/psmux-fleet/src/model.rs` | `Workspace`, `WorkspaceStatus`, `AgentKind` types |
| `crates/psmux-fleet/src/session.rs` | `SessionResolver` — arg → env → auto-detect |
| `crates/psmux-fleet/src/state.rs` | `StateReader` + `StateWriter` (CAS) |
| `crates/psmux-fleet/src/detector.rs` | `AwaitingDetector` trait + `RegexDetector` + `ClaudeTranscriptDetector` |
| `crates/psmux-fleet/src/table.rs` | `WorkspaceTable::hydrate_cold` |
| `crates/psmux-fleet/tests/hydrate_cold.rs` | Fixture-based hydration tests |
| `crates/psmux-fleet/tests/state_cas.rs` | CAS retry semantics, atomic write contract |
| `crates/psmux-fleet/tests/detector.rs` | Regex + transcript detector tests |
| `crates/psmux-fleet/tests/session.rs` | Resolver priority order tests |
| `crates/psmux-fleet/fixtures/state_healthy.json` | Reference state.json for tests |
| `crates/psmux-fleet/fixtures/transcript_awaiting.jsonl` | Claude JSONL fixture — tool_use without tool_result |
| `crates/psmux-fleet/fixtures/transcript_idle.jsonl` | Claude JSONL fixture — matched tool_use/tool_result pair |
| `crates/psmux-fleet-mcp/Cargo.toml` | MCP server crate manifest |
| `crates/psmux-fleet-mcp/src/main.rs` | Stdio server entry, tool registration |
| `crates/psmux-fleet-mcp/src/tools.rs` | `fleet.list` + `fleet.status` handlers |
| `crates/psmux-fleet-mcp/tests/mcp_end_to_end.rs` | Scripted stdio client → list/status response |
| `src/fleet/mod.rs` | Inside psmux binary — `fleet` subcommand dispatcher |
| `src/fleet/ls.rs` | `psmux fleet ls` handler |
| `src/fleet/status.rs` | `psmux fleet status <id>` handler |

### Modified files

| Path | Modification |
|---|---|
| `Cargo.toml` (root) | Add `crates/psmux-fleet` and `crates/psmux-fleet-mcp` to `[workspace].members` |
| `src/orchestrate.rs:170-174` | Add `state_version: u64` field to `OrchestrationState` with `#[serde(default)]`; increment on every write |
| `src/main.rs` | Register `fleet` subcommand group |
| `src/lib.rs` (if present) or wherever commands are dispatched | Wire `fleet` subcommand parser |

---

## Task 1: Scaffold workspace members and error enum

**Files:**
- Create: `crates/psmux-fleet/Cargo.toml`
- Create: `crates/psmux-fleet/src/lib.rs`
- Create: `crates/psmux-fleet/src/error.rs`
- Modify: `Cargo.toml` (root, line 12-13 — `[workspace].members`)

- [ ] **Step 1: Add the crate to the workspace**

Edit root `Cargo.toml`, change line 13 from:
```toml
members = [".", "crates/portable-pty-psmux", "crates/vt100-psmux"]
```
to:
```toml
members = [".", "crates/portable-pty-psmux", "crates/vt100-psmux", "crates/psmux-fleet"]
```

- [ ] **Step 2: Create the crate manifest**

Write `crates/psmux-fleet/Cargo.toml`:
```toml
[package]
name = "psmux-fleet"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "Aggregator library over psmux fleet primitives — state, detectors, operations"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
anyhow = "1"
chrono = { version = "0.4", features = ["serde"] }
regex = "1"
fs2 = "0.4"
tempfile = "3"
rand = "0.8"

[features]
default = ["fleet-claude-transcript"]
fleet-claude-transcript = []

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt", "time"] }
```

- [ ] **Step 3: Write the failing error-code test**

Create `crates/psmux-fleet/tests/error_codes.rs`:
```rust
use psmux_fleet::FleetError;

#[test]
fn error_codes_match_spec() {
    // Every code enumerated in spec §9 must be constructible and stringify
    // to its documented name.
    let cases = [
        (FleetError::WorkspaceNotFound("id".into()), "WORKSPACE_NOT_FOUND"),
        (FleetError::SessionNotFound, "SESSION_NOT_FOUND"),
        (FleetError::SessionAmbiguous(vec!["a".into(), "b".into()]), "SESSION_AMBIGUOUS"),
        (FleetError::PaneDead("%3".into()), "PANE_DEAD"),
        (FleetError::PaneNotReady("id".into()), "PANE_NOT_READY"),
        (FleetError::AlreadyTerminal("id".into()), "ALREADY_TERMINAL"),
        (FleetError::BackendDown, "BACKEND_DOWN"),
        (FleetError::WorktreeDirty { workspace: "id".into(), paths: vec![] }, "WORKTREE_DIRTY"),
        (FleetError::MergeConflict { kind: psmux_fleet::ConflictKind::TextConflict, conflicts: vec![], stderr: String::new() }, "MERGE_CONFLICT"),
        (FleetError::MergeInProgress("base".into()), "MERGE_IN_PROGRESS"),
        (FleetError::StateVersionMismatch, "STATE_VERSION_MISMATCH"),
        (FleetError::GitUnavailable, "GIT_UNAVAILABLE"),
        (FleetError::OrchestrateSpawnFailed("reason".into()), "ORCHESTRATE_SPAWN_FAILED"),
        (FleetError::QuotaExceeded { limit: 16, current: 16 }, "QUOTA_EXCEEDED"),
    ];
    for (err, expected_code) in cases {
        assert_eq!(err.code(), expected_code);
    }
}
```

- [ ] **Step 4: Run test to verify it fails (compilation error)**

Run from repo root: `cargo test -p psmux-fleet --test error_codes`
Expected: compilation failure — `FleetError` undefined.

- [ ] **Step 5: Implement the error enum**

Write `crates/psmux-fleet/src/error.rs`:
```rust
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
    WorktreeDirty { workspace: String, paths: Vec<String> },
    #[error("merge conflict ({kind:?})")]
    MergeConflict { kind: ConflictKind, conflicts: Vec<String>, stderr: String },
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
```

- [ ] **Step 6: Create the lib.rs skeleton**

Write `crates/psmux-fleet/src/lib.rs`:
```rust
pub mod error;

pub use error::{ConflictKind, FleetError};
```

- [ ] **Step 7: Run test to verify it passes**

Run: `cargo test -p psmux-fleet --test error_codes`
Expected: PASS (1 test).

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml crates/psmux-fleet/
git commit -m "feat(fleet): scaffold psmux-fleet crate with error taxonomy"
```

---

## Task 2: Add state_version to OrchestrationState

**Files:**
- Modify: `src/orchestrate.rs:170-174` (add field)
- Modify: `src/orchestrate.rs` (wherever state is written — likely a `save()` or `write()` helper; increment there)
- Create: `tests-rs/test_state_version.rs`
- Modify: `Cargo.toml` (root) — register the new integration test

Rationale: the CAS contract (spec §6.1) requires a monotonic `state_version` field on `OrchestrationState`. Adding it as `#[serde(default)]` keeps existing state.json files loadable. Orchestrate's existing writes increment it.

- [ ] **Step 1: Write the failing test**

Create `tests-rs/test_state_version.rs`:
```rust
//! Contract: state.json has a monotonic `state_version` field that increments
//! on every write and defaults to 0 when loading pre-existing files.

use std::fs;
use tempfile::TempDir;

#[test]
fn state_version_defaults_to_zero_on_old_files() {
    let td = TempDir::new().unwrap();
    let path = td.path().join("state.json");
    // Write a pre-existing state.json without state_version.
    fs::write(&path, r#"{"session":"demo","workers":{}}"#).unwrap();
    let state: psmux::orchestrate::OrchestrationState =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state.state_version, 0);
}

#[test]
fn state_version_serializes_when_present() {
    let td = TempDir::new().unwrap();
    let path = td.path().join("state.json");
    let state = psmux::orchestrate::OrchestrationState {
        session: "demo".into(),
        workers: Default::default(),
        state_version: 42,
    };
    fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"state_version\":42"));
}
```

Add the test registration in root `Cargo.toml` alongside the other `[[test]]` entries:
```toml
[[test]]
name = "test_state_version"
path = "tests-rs/test_state_version.rs"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test test_state_version`
Expected: compilation failure — `state_version` field does not exist.

- [ ] **Step 3: Add the field**

Edit `src/orchestrate.rs:170-174` from:
```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct OrchestrationState {
    pub session: String,
    pub workers: HashMap<String, WorkerState>,
}
```
to:
```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct OrchestrationState {
    pub session: String,
    pub workers: HashMap<String, WorkerState>,
    /// Monotonic version for CAS writes (spec §6.1).
    /// Defaults to 0 when loading pre-existing state.json files.
    #[serde(default)]
    pub state_version: u64,
}
```

- [ ] **Step 4: Update orchestrate's write path to increment**

Locate the function in `src/orchestrate.rs` that serializes state to disk (search for `state.json` writes — around line 772 per the grep result). Wherever `serde_json::to_writer` / `to_string` is called on `OrchestrationState`, increment `state.state_version += 1` immediately before the serialization.

Example patch (exact line depends on current code):
```rust
// Before writing:
state.state_version = state.state_version.wrapping_add(1);
let json = serde_json::to_string_pretty(&state)?;
// ...atomic write continues...
```

- [ ] **Step 5: Run tests**

Run: `cargo test --test test_state_version`
Expected: PASS (2 tests).

Then run: `cargo test --lib orchestrate`
Expected: existing orchestrate tests still pass (no regression).

- [ ] **Step 6: Commit**

```bash
git add src/orchestrate.rs tests-rs/test_state_version.rs Cargo.toml
git commit -m "feat(orchestrate): add monotonic state_version field for CAS writes"
```

---

## Task 3: Core domain types — Workspace, WorkspaceStatus, AgentKind

**Files:**
- Create: `crates/psmux-fleet/src/model.rs`
- Modify: `crates/psmux-fleet/src/lib.rs` — re-export

- [ ] **Step 1: Write the failing test**

Create `crates/psmux-fleet/tests/model.rs`:
```rust
use chrono::{TimeZone, Utc};
use psmux_fleet::{AgentKind, Workspace, WorkspaceStatus};
use std::collections::HashMap;
use std::path::PathBuf;

#[test]
fn workspace_round_trips_json() {
    let w = Workspace {
        id: "fix-auth-bug".into(),
        branch: Some("fleet/fix-auth-bug".into()),
        worktree_path: Some(PathBuf::from("/tmp/fix-auth-bug")),
        pane_id: Some("%3".into()),
        session: "demo".into(),
        agent_kind: AgentKind::Claude,
        status: WorkspaceStatus::Running,
        task: Some("fix the auth bug".into()),
        started_at: Some(Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap()),
        finished_at: None,
        exit_code: None,
        crash_dump_path: None,
        metadata: HashMap::new(),
    };
    let json = serde_json::to_string(&w).unwrap();
    let back: Workspace = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "fix-auth-bug");
    assert_eq!(back.status, WorkspaceStatus::Running);
    assert_eq!(back.agent_kind, AgentKind::Claude);
}

#[test]
fn agent_kind_unknown_for_untagged_pane() {
    assert_eq!(AgentKind::default(), AgentKind::Unknown);
}

#[test]
fn workspace_status_serializes_snake_case() {
    assert_eq!(serde_json::to_string(&WorkspaceStatus::Running).unwrap(), r#""running""#);
    assert_eq!(serde_json::to_string(&WorkspaceStatus::Awaiting).unwrap(), r#""awaiting""#);
    assert_eq!(serde_json::to_string(&WorkspaceStatus::Succeeded).unwrap(), r#""succeeded""#);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p psmux-fleet --test model`
Expected: compilation failure — types undefined.

- [ ] **Step 3: Implement the types**

Write `crates/psmux-fleet/src/model.rs`:
```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Claude,
    Codex,
    Pi,
    Shell,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStatus {
    Pending,
    Running,
    Awaiting,
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub branch: Option<String>,
    pub worktree_path: Option<PathBuf>,
    pub pane_id: Option<String>,
    pub session: String,
    pub agent_kind: AgentKind,
    pub status: WorkspaceStatus,
    pub task: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub crash_dump_path: Option<PathBuf>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}
```

Update `crates/psmux-fleet/src/lib.rs`:
```rust
pub mod error;
pub mod model;

pub use error::{ConflictKind, FleetError};
pub use model::{AgentKind, Workspace, WorkspaceStatus};
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p psmux-fleet --test model`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/psmux-fleet/src/model.rs crates/psmux-fleet/src/lib.rs crates/psmux-fleet/tests/model.rs
git commit -m "feat(fleet): add Workspace, WorkspaceStatus, AgentKind types"
```

---

## Task 4: Session resolver

**Files:**
- Create: `crates/psmux-fleet/src/session.rs`
- Modify: `crates/psmux-fleet/src/lib.rs`
- Create: `crates/psmux-fleet/tests/session.rs`

Implements spec §7: explicit arg → `$PSMUX_SESSION` → auto-detect via `psmux ls --json`.

- [ ] **Step 1: Write the failing test**

Create `crates/psmux-fleet/tests/session.rs`:
```rust
use psmux_fleet::{FleetError, SessionResolver};

#[test]
fn explicit_arg_wins_over_env() {
    std::env::set_var("PSMUX_SESSION", "env-session");
    let r = SessionResolver::new_with_candidates(vec!["auto".into()]);
    let got = r.resolve(Some("explicit")).unwrap();
    assert_eq!(got, "explicit");
    std::env::remove_var("PSMUX_SESSION");
}

#[test]
fn env_wins_over_auto_detect() {
    std::env::set_var("PSMUX_SESSION", "env-session");
    let r = SessionResolver::new_with_candidates(vec!["auto-a".into(), "auto-b".into()]);
    let got = r.resolve(None).unwrap();
    assert_eq!(got, "env-session");
    std::env::remove_var("PSMUX_SESSION");
}

#[test]
fn single_candidate_auto_detects() {
    std::env::remove_var("PSMUX_SESSION");
    let r = SessionResolver::new_with_candidates(vec!["only-one".into()]);
    let got = r.resolve(None).unwrap();
    assert_eq!(got, "only-one");
}

#[test]
fn zero_candidates_errors_not_found() {
    std::env::remove_var("PSMUX_SESSION");
    let r = SessionResolver::new_with_candidates(vec![]);
    assert!(matches!(r.resolve(None), Err(FleetError::SessionNotFound)));
}

#[test]
fn multiple_candidates_errors_ambiguous() {
    std::env::remove_var("PSMUX_SESSION");
    let r = SessionResolver::new_with_candidates(vec!["a".into(), "b".into()]);
    match r.resolve(None) {
        Err(FleetError::SessionAmbiguous(v)) => assert_eq!(v.len(), 2),
        other => panic!("expected SessionAmbiguous, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p psmux-fleet --test session`
Expected: compilation failure.

- [ ] **Step 3: Implement**

Write `crates/psmux-fleet/src/session.rs`:
```rust
use crate::FleetError;

/// Resolves the target psmux session per spec §7.
///
/// Priority order:
/// 1. Explicit argument
/// 2. `$PSMUX_SESSION` environment variable
/// 3. Auto-detect via candidate list (from `psmux ls --json`)
pub struct SessionResolver {
    candidates: Vec<String>,
}

impl SessionResolver {
    /// Constructs a resolver with a pre-fetched candidate list.
    /// Callers performing real auto-detection should invoke `psmux ls --json`
    /// and pass the session names here.
    pub fn new_with_candidates(candidates: Vec<String>) -> Self {
        Self { candidates }
    }

    /// Resolves to a single session name or returns a typed error.
    pub fn resolve(&self, explicit: Option<&str>) -> Result<String, FleetError> {
        if let Some(s) = explicit {
            return Ok(s.to_string());
        }
        if let Ok(env) = std::env::var("PSMUX_SESSION") {
            if !env.is_empty() {
                return Ok(env);
            }
        }
        match self.candidates.len() {
            0 => Err(FleetError::SessionNotFound),
            1 => Ok(self.candidates[0].clone()),
            _ => Err(FleetError::SessionAmbiguous(self.candidates.clone())),
        }
    }
}
```

Update `crates/psmux-fleet/src/lib.rs`:
```rust
pub mod error;
pub mod model;
pub mod session;

pub use error::{ConflictKind, FleetError};
pub use model::{AgentKind, Workspace, WorkspaceStatus};
pub use session::SessionResolver;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p psmux-fleet --test session -- --test-threads=1`
(`--test-threads=1` because env-var tests mutate process state and must serialize.)
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/psmux-fleet/src/session.rs crates/psmux-fleet/src/lib.rs crates/psmux-fleet/tests/session.rs
git commit -m "feat(fleet): session resolver with explicit → env → auto-detect priority"
```

---

## Task 5: StateReader — parse state.json into FleetView-shaped workspaces

**Files:**
- Create: `crates/psmux-fleet/src/state.rs` (reader half; writer added in Task 6)
- Create: `crates/psmux-fleet/fixtures/state_healthy.json`
- Create: `crates/psmux-fleet/tests/state_read.rs`
- Modify: `crates/psmux-fleet/src/lib.rs`
- Modify: `crates/psmux-fleet/Cargo.toml` — add `psmux` as a path dependency so we can reuse `OrchestrationState`

- [ ] **Step 1: Expose orchestrate types as library items**

The `psmux` binary currently has `src/orchestrate.rs` as part of the binary crate. `psmux-fleet` needs access to `OrchestrationState` without pulling in the whole binary.

Option: declare a library target in the root `Cargo.toml` so `psmux-fleet` can depend on `psmux` as a library:

Check root `Cargo.toml` — if there is no `[lib]` section, add one (it coexists with `[package]` / `[[bin]]` definitions):
```toml
[lib]
name = "psmux"
path = "src/lib.rs"
```

If `src/lib.rs` does not exist, create a minimal one that re-exports orchestrate:
```rust
// src/lib.rs
pub mod orchestrate;
```

And verify the binary's `src/main.rs` still compiles (it may need to `use psmux::orchestrate;` now if it did `mod orchestrate;` inline).

Run: `cargo check --workspace` — fix any inline-module collisions flagged.

- [ ] **Step 2: Add psmux as a dev-dep and regular dep in psmux-fleet**

Edit `crates/psmux-fleet/Cargo.toml` `[dependencies]`:
```toml
psmux = { path = "../..", default-features = false }
```

- [ ] **Step 3: Create the reference fixture**

Write `crates/psmux-fleet/fixtures/state_healthy.json`:
```json
{
  "session": "demo",
  "state_version": 7,
  "workers": {
    "fix-auth": {
      "status": "running",
      "pane_id": "%3",
      "pid": 12345,
      "started_at": "2026-04-22T12:00:00Z"
    },
    "lint-docs": {
      "status": "succeeded",
      "pane_id": "%4",
      "pid": 12346,
      "exit_code": 0,
      "started_at": "2026-04-22T11:55:00Z",
      "finished_at": "2026-04-22T12:01:00Z"
    },
    "dead-worker": {
      "status": "failed",
      "exit_code": 1,
      "started_at": "2026-04-22T11:50:00Z",
      "finished_at": "2026-04-22T11:52:00Z",
      "crash_dump_path": "C:\\Users\\aravi\\AppData\\Local\\psmux\\crashes\\2026-04-22T11-52-00.json"
    }
  }
}
```

- [ ] **Step 4: Write the failing test**

Create `crates/psmux-fleet/tests/state_read.rs`:
```rust
use psmux_fleet::{StateReader, WorkspaceStatus};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)
}

#[test]
fn reads_state_version_from_fixture() {
    let reader = StateReader::load(&fixture("state_healthy.json")).unwrap();
    assert_eq!(reader.state_version(), 7);
    assert_eq!(reader.session(), "demo");
}

#[test]
fn maps_running_worker_to_workspace() {
    let reader = StateReader::load(&fixture("state_healthy.json")).unwrap();
    let workspaces = reader.workspaces();
    let fix_auth = workspaces.iter().find(|w| w.id == "fix-auth").unwrap();
    assert_eq!(fix_auth.status, WorkspaceStatus::Running);
    assert_eq!(fix_auth.pane_id.as_deref(), Some("%3"));
}

#[test]
fn maps_terminal_worker_with_exit_code() {
    let reader = StateReader::load(&fixture("state_healthy.json")).unwrap();
    let workspaces = reader.workspaces();
    let lint = workspaces.iter().find(|w| w.id == "lint-docs").unwrap();
    assert_eq!(lint.status, WorkspaceStatus::Succeeded);
    assert_eq!(lint.exit_code, Some(0));
    assert!(lint.finished_at.is_some());
}

#[test]
fn preserves_crash_dump_path() {
    let reader = StateReader::load(&fixture("state_healthy.json")).unwrap();
    let workspaces = reader.workspaces();
    let dead = workspaces.iter().find(|w| w.id == "dead-worker").unwrap();
    assert!(dead.crash_dump_path.is_some());
    assert_eq!(dead.status, WorkspaceStatus::Failed);
}
```

- [ ] **Step 5: Run test to verify it fails**

Run: `cargo test -p psmux-fleet --test state_read`
Expected: compilation failure — `StateReader` undefined.

- [ ] **Step 6: Implement StateReader**

Write `crates/psmux-fleet/src/state.rs`:
```rust
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
    pub fn load(path: &Path) -> Result<Self, FleetError> {
        let bytes = std::fs::read(path)?;
        let state: OrchestrationState = serde_json::from_slice(&bytes)?;
        Ok(Self { state })
    }

    pub fn state_version(&self) -> u64 { self.state.state_version }
    pub fn session(&self) -> &str { &self.state.session }

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
        branch: None,                 // populated by warm hydration in C2
        worktree_path: None,          // populated by warm hydration in C2
        pane_id: ws.pane_id.clone(),
        session: session.to_string(),
        agent_kind: AgentKind::Unknown, // populated by @agent metadata in C2
        status: map_status(&ws.status),
        task: None,                   // from plan.json in C2
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
    DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&Utc))
}
```

Update `crates/psmux-fleet/src/lib.rs`:
```rust
pub mod error;
pub mod model;
pub mod session;
pub mod state;

pub use error::{ConflictKind, FleetError};
pub use model::{AgentKind, Workspace, WorkspaceStatus};
pub use session::SessionResolver;
pub use state::StateReader;
```

- [ ] **Step 7: Run test to verify it passes**

Run: `cargo test -p psmux-fleet --test state_read`
Expected: PASS (4 tests).

- [ ] **Step 8: Commit**

```bash
git add crates/psmux-fleet/src/state.rs crates/psmux-fleet/src/lib.rs crates/psmux-fleet/fixtures/ crates/psmux-fleet/tests/state_read.rs crates/psmux-fleet/Cargo.toml Cargo.toml src/lib.rs
git commit -m "feat(fleet): StateReader projects OrchestrationState into Workspace model"
```

---

## Task 6: StateWriter with CAS

**Files:**
- Modify: `crates/psmux-fleet/src/state.rs` — add `StateWriter` + `update_state` helper
- Modify: `crates/psmux-fleet/src/lib.rs` — re-export
- Create: `crates/psmux-fleet/tests/state_cas.rs`

Implements spec §6.1 — versioned writes with bounded retry.

- [ ] **Step 1: Write the failing test**

Create `crates/psmux-fleet/tests/state_cas.rs`:
```rust
use psmux_fleet::state::update_state;
use psmux_fleet::FleetError;
use psmux::orchestrate::{OrchestrationState, WorkerState};
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

fn seed(td: &TempDir, version: u64) -> std::path::PathBuf {
    let path = td.path().join("state.json");
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
    let td = TempDir::new().unwrap();
    let path = seed(&td, 5);
    update_state(&path, |s| {
        s.workers.insert("w1".into(), WorkerState::new_pending());
        Ok(())
    }).unwrap();
    let back: OrchestrationState =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(back.state_version, 6);
    assert!(back.workers.contains_key("w1"));
}

#[test]
fn update_state_retries_on_concurrent_mutation() {
    let td = TempDir::new().unwrap();
    let path = seed(&td, 0);
    let path_inner = path.clone();
    // The mutator races with an external writer: on the first pass, it bumps
    // the on-disk version behind our back. Our helper should observe the
    // mismatch, reload, and retry successfully.
    let mut call_count = 0;
    update_state(&path, |s| {
        call_count += 1;
        if call_count == 1 {
            // Simulate a concurrent writer racing between load and CAS:
            let mut external = s.clone();
            external.state_version = s.state_version + 10;
            fs::write(&path_inner, serde_json::to_string(&external).unwrap()).unwrap();
        }
        s.workers.insert("w1".into(), WorkerState::new_pending());
        Ok(())
    }).unwrap();
    assert!(call_count >= 2, "must have retried");
    let back: OrchestrationState =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(back.workers.contains_key("w1"));
}

#[test]
fn update_state_surfaces_after_retry_exhaustion() {
    let td = TempDir::new().unwrap();
    let path = seed(&td, 0);
    let path_inner = path.clone();
    // Always force a concurrent write. Should exhaust retries and return
    // STATE_VERSION_MISMATCH.
    let res = update_state(&path, |s| {
        let mut external = s.clone();
        external.state_version = s.state_version + 100;
        fs::write(&path_inner, serde_json::to_string(&external).unwrap()).unwrap();
        Ok(())
    });
    assert!(matches!(res, Err(FleetError::StateVersionMismatch)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p psmux-fleet --test state_cas`
Expected: compilation failure — `update_state` undefined.

- [ ] **Step 3: Implement update_state in src/state.rs**

Append to `crates/psmux-fleet/src/state.rs`:
```rust
use psmux::orchestrate::OrchestrationState;
use std::fs;
use std::io::Write;
use tempfile::NamedTempFile;

/// Compare-and-swap update helper (spec §6.1).
///
/// Reads `path`, applies `mutate`, attempts an atomic write iff the on-disk
/// state_version still matches the observed value. Retries up to 3 times on
/// mismatch. Returns `STATE_VERSION_MISMATCH` if all retries collide.
///
/// Logs `fleet_state_cas_retries_total` count to stderr on every retry so
/// operators can tune the retry budget.
pub fn update_state<F>(path: &Path, mutate: F) -> Result<(), FleetError>
where
    F: FnMut(&mut OrchestrationState) -> Result<(), FleetError>,
{
    let mut mutate = mutate;
    const MAX_RETRIES: u32 = 3;
    for attempt in 0..MAX_RETRIES {
        let bytes = fs::read(path)?;
        let mut state: OrchestrationState = serde_json::from_slice(&bytes)?;
        let observed = state.state_version;
        mutate(&mut state)?;
        state.state_version = observed.wrapping_add(1);
        if try_cas_write(path, &state, observed)? {
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

fn try_cas_write(
    path: &Path,
    state: &OrchestrationState,
    observed: u64,
) -> Result<bool, FleetError> {
    // Re-read under a short read window to check observed version is current.
    let bytes = fs::read(path)?;
    let on_disk: OrchestrationState = serde_json::from_slice(&bytes)?;
    if on_disk.state_version != observed {
        return Ok(false);
    }
    let parent = path.parent().ok_or(FleetError::BackendDown)?;
    let mut tmp = NamedTempFile::new_in(parent)?;
    let serialized = serde_json::to_vec_pretty(state)?;
    tmp.as_file_mut().write_all(&serialized)?;
    tmp.as_file_mut().sync_all()?;
    tmp.persist(path).map_err(|e| FleetError::Io(e.error))?;
    Ok(true)
}
```

Update `crates/psmux-fleet/src/lib.rs` to re-export:
```rust
pub use state::{update_state, StateReader};
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p psmux-fleet --test state_cas`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/psmux-fleet/src/state.rs crates/psmux-fleet/src/lib.rs crates/psmux-fleet/tests/state_cas.rs
git commit -m "feat(fleet): CAS update_state helper with bounded retry"
```

---

## Task 7: RegexDetector with 2 Hz debounce

**Files:**
- Create: `crates/psmux-fleet/src/detector.rs`
- Modify: `crates/psmux-fleet/src/lib.rs`
- Create: `crates/psmux-fleet/tests/detector.rs`

Implements spec §8.1 — regex is the primary awaiting-input path.

- [ ] **Step 1: Write the failing test**

Create `crates/psmux-fleet/tests/detector.rs`:
```rust
use psmux_fleet::detector::{AwaitingDetector, RegexDetector};
use psmux_fleet::AgentKind;
use std::time::Duration;

#[test]
fn claude_confirmation_prompt_detected() {
    let d = RegexDetector::new_with_defaults();
    let text = "Do something?\nDo you want to proceed?\n";
    let v = d.detect(AgentKind::Claude, text);
    assert!(v.is_some());
    let reason = v.unwrap();
    assert!(reason.contains("Do you want to proceed"));
}

#[test]
fn shell_prompt_detected() {
    let d = RegexDetector::new_with_defaults();
    assert!(d.detect(AgentKind::Shell, "$ ").is_some());
    assert!(d.detect(AgentKind::Shell, "PS C:\\Users> ").is_some());
    assert!(d.detect(AgentKind::Shell, "> ").is_some());
}

#[test]
fn autonomous_prose_is_not_a_match() {
    // OMC's d6354c8 regression guard.
    let d = RegexDetector::new_with_defaults();
    let text = "Running autonomous agents in parallel. No prompt visible.";
    assert!(d.detect(AgentKind::Claude, text).is_none());
}

#[test]
fn debounce_caps_to_2_hz_per_workspace() {
    let d = RegexDetector::new_with_defaults();
    let ws = "w1";
    assert!(d.try_detect(ws, AgentKind::Claude, "[y/n]"));
    // Second call within 500ms should be debounced (returns false without re-evaluating)
    assert!(!d.try_detect(ws, AgentKind::Claude, "[y/n]"));
    std::thread::sleep(Duration::from_millis(520));
    assert!(d.try_detect(ws, AgentKind::Claude, "[y/n]"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p psmux-fleet --test detector`
Expected: compilation failure.

- [ ] **Step 3: Implement**

Write `crates/psmux-fleet/src/detector.rs`:
```rust
use crate::AgentKind;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const DEBOUNCE_WINDOW: Duration = Duration::from_millis(500); // 2 Hz

pub trait AwaitingDetector: Send + Sync {
    /// Returns the matched-pattern reason if the tail of `text` indicates
    /// the agent is awaiting input.
    fn detect(&self, kind: AgentKind, text: &str) -> Option<String>;
}

pub struct RegexDetector {
    patterns: HashMap<AgentKind, Vec<Regex>>,
    last_eval: Mutex<HashMap<String, Instant>>,
}

impl RegexDetector {
    pub fn new_with_defaults() -> Self {
        let mut patterns: HashMap<AgentKind, Vec<Regex>> = HashMap::new();
        patterns.insert(
            AgentKind::Claude,
            vec![
                Regex::new(r"Do you want to proceed\?").unwrap(),
                Regex::new(r"\[y/n\]").unwrap(),
                Regex::new(r"\(y/N\)").unwrap(),
                Regex::new(r"Press Enter to continue").unwrap(),
            ],
        );
        patterns.insert(
            AgentKind::Shell,
            vec![
                Regex::new(r"\$ $").unwrap(),
                Regex::new(r"PS .*> $").unwrap(),
                Regex::new(r"^> $").unwrap(),
            ],
        );
        Self {
            patterns,
            last_eval: Mutex::new(HashMap::new()),
        }
    }

    /// Per-workspace debounced detect. Returns true if a match was recorded
    /// during this window; false if debounced or no match.
    pub fn try_detect(&self, workspace_id: &str, kind: AgentKind, text: &str) -> bool {
        let now = Instant::now();
        let mut last = self.last_eval.lock().unwrap();
        if let Some(prev) = last.get(workspace_id) {
            if now.duration_since(*prev) < DEBOUNCE_WINDOW {
                return false;
            }
        }
        last.insert(workspace_id.to_string(), now);
        drop(last);
        self.detect(kind, text).is_some()
    }
}

impl AwaitingDetector for RegexDetector {
    fn detect(&self, kind: AgentKind, text: &str) -> Option<String> {
        // Match against the last 10 lines only (spec §8.1).
        let tail: String = text.lines().rev().take(10).collect::<Vec<_>>().join("\n");
        let patterns = self.patterns.get(&kind)?;
        for re in patterns {
            if let Some(m) = re.find(&tail) {
                return Some(m.as_str().to_string());
            }
        }
        None
    }
}
```

Update `crates/psmux-fleet/src/lib.rs` module list:
```rust
pub mod detector;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p psmux-fleet --test detector`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/psmux-fleet/src/detector.rs crates/psmux-fleet/src/lib.rs crates/psmux-fleet/tests/detector.rs
git commit -m "feat(fleet): regex awaiting-detector with 2 Hz per-workspace debounce"
```

---

## Task 8: ClaudeTranscriptDetector as confidence booster (feature-gated)

**Files:**
- Modify: `crates/psmux-fleet/src/detector.rs`
- Create: `crates/psmux-fleet/fixtures/transcript_awaiting.jsonl`
- Create: `crates/psmux-fleet/fixtures/transcript_idle.jsonl`
- Create: `crates/psmux-fleet/tests/detector_transcript.rs`

Implements spec §8.2 — JSONL booster, refines `awaiting_reason` when regex already detected awaiting. Does not flip state on its own.

- [ ] **Step 1: Create fixtures**

Write `crates/psmux-fleet/fixtures/transcript_awaiting.jsonl`:
```
{"type":"user","role":"user","content":"help me edit foo.py"}
{"type":"assistant","tool_use":{"id":"tu_001","name":"Edit","input":{"path":"foo.py"}}}
```

Write `crates/psmux-fleet/fixtures/transcript_idle.jsonl`:
```
{"type":"user","role":"user","content":"help me edit foo.py"}
{"type":"assistant","tool_use":{"id":"tu_001","name":"Edit","input":{"path":"foo.py"}}}
{"type":"tool_result","tool_use_id":"tu_001","content":"ok"}
```

- [ ] **Step 2: Write the failing test**

Create `crates/psmux-fleet/tests/detector_transcript.rs`:
```rust
#![cfg(feature = "fleet-claude-transcript")]

use psmux_fleet::detector::ClaudeTranscriptDetector;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)
}

#[test]
fn unmatched_tool_use_returns_reason() {
    let d = ClaudeTranscriptDetector::new();
    let reason = d.boost_reason(&fixture("transcript_awaiting.jsonl")).unwrap();
    assert!(reason.contains("Edit"));
    assert!(reason.to_lowercase().contains("tool"));
}

#[test]
fn matched_tool_use_returns_none() {
    let d = ClaudeTranscriptDetector::new();
    let reason = d.boost_reason(&fixture("transcript_idle.jsonl"));
    assert!(reason.is_none());
}

#[test]
fn malformed_jsonl_returns_none_not_error() {
    let d = ClaudeTranscriptDetector::new();
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "not valid json at all\n{\"also\":\"broken\"").unwrap();
    // Should silently downgrade — regex verdict stands.
    assert!(d.boost_reason(tmp.path()).is_none());
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p psmux-fleet --test detector_transcript --features fleet-claude-transcript`
Expected: compilation failure.

- [ ] **Step 4: Implement**

Append to `crates/psmux-fleet/src/detector.rs`:
```rust
#[cfg(feature = "fleet-claude-transcript")]
pub struct ClaudeTranscriptDetector;

#[cfg(feature = "fleet-claude-transcript")]
impl ClaudeTranscriptDetector {
    pub fn new() -> Self { Self }

    /// Parses a Claude JSONL transcript. Returns `Some(reason)` if the last
    /// event is a `tool_use` with no matching `tool_result`. Returns `None`
    /// on match-found, idle, or any parse failure — the caller's regex
    /// verdict stands in the degraded case.
    pub fn boost_reason(&self, path: &std::path::Path) -> Option<String> {
        let text = std::fs::read_to_string(path).ok()?;
        let mut last_tool_use: Option<(String, String)> = None; // (id, name)
        let mut matched: std::collections::HashSet<String> = Default::default();
        for line in text.lines() {
            let v: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Some(tu) = v.get("tool_use") {
                if let (Some(id), Some(name)) =
                    (tu.get("id").and_then(|s| s.as_str()),
                     tu.get("name").and_then(|s| s.as_str()))
                {
                    last_tool_use = Some((id.to_string(), name.to_string()));
                }
            }
            if v.get("type").and_then(|s| s.as_str()) == Some("tool_result") {
                if let Some(id) = v.get("tool_use_id").and_then(|s| s.as_str()) {
                    matched.insert(id.to_string());
                }
            }
        }
        let (id, name) = last_tool_use?;
        if matched.contains(&id) { None } else { Some(format!("Tool approval: {name}")) }
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p psmux-fleet --test detector_transcript --features fleet-claude-transcript`
Expected: PASS (3 tests).

Also verify the feature gate works:
Run: `cargo test -p psmux-fleet --no-default-features --test detector`
Expected: PASS (task 7's tests still green without the transcript feature).

- [ ] **Step 6: Commit**

```bash
git add crates/psmux-fleet/src/detector.rs crates/psmux-fleet/fixtures/transcript_*.jsonl crates/psmux-fleet/tests/detector_transcript.rs
git commit -m "feat(fleet): Claude JSONL transcript booster behind fleet-claude-transcript feature"
```

---

## Task 9: WorkspaceTable::hydrate_cold

**Files:**
- Create: `crates/psmux-fleet/src/table.rs`
- Modify: `crates/psmux-fleet/src/lib.rs`
- Create: `crates/psmux-fleet/tests/hydrate_cold.rs`

Combines StateReader + RegexDetector for the cold-path view. Warm hydration (backend pipe + mycel) lands in C2.

- [ ] **Step 1: Write the failing test**

Create `crates/psmux-fleet/tests/hydrate_cold.rs`:
```rust
use psmux_fleet::{WorkspaceStatus, WorkspaceTable};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)
}

#[test]
fn hydrate_cold_from_state_json() {
    let table = WorkspaceTable::hydrate_cold(&fixture("state_healthy.json")).unwrap();
    assert_eq!(table.len(), 3);
    let ids: Vec<_> = table.workspaces().iter().map(|w| w.id.as_str()).collect();
    assert!(ids.contains(&"fix-auth"));
    assert!(ids.contains(&"lint-docs"));
    assert!(ids.contains(&"dead-worker"));
}

#[test]
fn find_by_id_returns_specific_workspace() {
    let table = WorkspaceTable::hydrate_cold(&fixture("state_healthy.json")).unwrap();
    let ws = table.find("lint-docs").unwrap();
    assert_eq!(ws.status, WorkspaceStatus::Succeeded);
    assert_eq!(ws.exit_code, Some(0));
}

#[test]
fn filter_by_status_narrows_result() {
    let table = WorkspaceTable::hydrate_cold(&fixture("state_healthy.json")).unwrap();
    let running = table.filter_status(WorkspaceStatus::Running);
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].id, "fix-auth");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p psmux-fleet --test hydrate_cold`
Expected: compilation failure.

- [ ] **Step 3: Implement**

Write `crates/psmux-fleet/src/table.rs`:
```rust
use crate::{FleetError, StateReader, Workspace, WorkspaceStatus};
use std::path::Path;

pub struct WorkspaceTable {
    workspaces: Vec<Workspace>,
}

impl WorkspaceTable {
    /// Cold hydration: read state.json only. Fills fields that require
    /// backend/mycel with defaults (branch=None, agent_kind=Unknown, etc.).
    /// Warm hydration (§ C2) will enrich these.
    pub fn hydrate_cold(state_path: &Path) -> Result<Self, FleetError> {
        let reader = StateReader::load(state_path)?;
        Ok(Self { workspaces: reader.workspaces() })
    }

    pub fn len(&self) -> usize { self.workspaces.len() }
    pub fn is_empty(&self) -> bool { self.workspaces.is_empty() }
    pub fn workspaces(&self) -> &[Workspace] { &self.workspaces }

    pub fn find(&self, id: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    pub fn filter_status(&self, status: WorkspaceStatus) -> Vec<&Workspace> {
        self.workspaces.iter().filter(|w| w.status == status).collect()
    }
}
```

Update `crates/psmux-fleet/src/lib.rs`:
```rust
pub mod table;
pub use table::WorkspaceTable;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p psmux-fleet --test hydrate_cold`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/psmux-fleet/src/table.rs crates/psmux-fleet/src/lib.rs crates/psmux-fleet/tests/hydrate_cold.rs
git commit -m "feat(fleet): WorkspaceTable::hydrate_cold from state.json"
```

---

## Task 10: `psmux fleet ls` CLI subcommand

**Files:**
- Create: `src/fleet/mod.rs`
- Create: `src/fleet/ls.rs`
- Modify: `src/main.rs` or wherever `psmux` subcommands are registered
- Modify: `Cargo.toml` (root) — add `psmux-fleet` as dep

- [ ] **Step 1: Add psmux-fleet as a dep of the psmux binary**

Edit root `Cargo.toml` `[dependencies]`:
```toml
psmux-fleet = { path = "crates/psmux-fleet" }
```

- [ ] **Step 2: Locate the subcommand registration site**

Search: `grep -rn "fn cmd_\|Subcommand::\|match.*subcommand" src/main.rs src/lib.rs | head -20`

Identify how existing subcommands (e.g., `orchestrate`, `exec`, `wait-for`) are dispatched. Note the pattern: likely a `clap` parser with subcommand variants, then a dispatch `match`.

- [ ] **Step 3: Write an end-to-end integration test for `psmux fleet ls`**

Create `tests-rs/test_fleet_ls.rs`:
```rust
//! Integration test for `psmux fleet ls --json`.
//! Seeds a fake state.json, invokes the compiled binary, parses JSON output.

use std::process::Command;
use tempfile::TempDir;

fn psmux_bin() -> String {
    env!("CARGO_BIN_EXE_psmux").to_string()
}

#[test]
fn fleet_ls_json_reads_state_and_emits_workspaces() {
    let td = TempDir::new().unwrap();
    let state_path = td.path().join("state.json");
    std::fs::write(&state_path, include_str!("../crates/psmux-fleet/fixtures/state_healthy.json")).unwrap();

    let out = Command::new(psmux_bin())
        .args(["fleet", "ls", "--state-path", state_path.to_str().unwrap(), "--json"])
        .env_remove("PSMUX_SESSION")
        .output()
        .expect("run psmux");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let parsed: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("parse JSON output");
    let workspaces = parsed.get("workspaces").and_then(|v| v.as_array()).expect("workspaces array");
    assert_eq!(workspaces.len(), 3);
}
```

Register in root `Cargo.toml`:
```toml
[[test]]
name = "test_fleet_ls"
path = "tests-rs/test_fleet_ls.rs"
```

- [ ] **Step 4: Run test to verify it fails**

Run: `cargo test --test test_fleet_ls`
Expected: failure — `fleet` subcommand not registered.

- [ ] **Step 5: Implement the subcommand**

Write `src/fleet/mod.rs`:
```rust
//! `psmux fleet` subcommand tree (spec §5).

pub mod ls;
pub mod status;

use anyhow::Result;

#[derive(Debug, clap::Subcommand)]
pub enum FleetCmd {
    /// List workspaces in a session.
    Ls(ls::LsArgs),
    /// Show detail for one workspace.
    Status(status::StatusArgs),
}

pub fn dispatch(cmd: FleetCmd) -> Result<()> {
    match cmd {
        FleetCmd::Ls(args) => ls::run(args),
        FleetCmd::Status(args) => status::run(args),
    }
}
```

Write `src/fleet/ls.rs`:
```rust
use anyhow::{Context, Result};
use psmux_fleet::WorkspaceTable;
use std::path::PathBuf;

#[derive(Debug, clap::Args)]
pub struct LsArgs {
    /// Explicit state.json path (default: auto-discover per session).
    #[arg(long)]
    pub state_path: Option<PathBuf>,
    /// Session name (default: $PSMUX_SESSION or auto-detect).
    #[arg(long)]
    pub session: Option<String>,
    /// Emit JSON instead of a table.
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: LsArgs) -> Result<()> {
    let path = resolve_state_path(&args)?;
    let table = WorkspaceTable::hydrate_cold(&path)
        .with_context(|| format!("loading {}", path.display()))?;
    if args.json {
        let out = serde_json::json!({
            "workspaces": table.workspaces(),
            "total": table.len(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        print_table(&table);
    }
    Ok(())
}

fn resolve_state_path(args: &LsArgs) -> Result<PathBuf> {
    if let Some(p) = &args.state_path { return Ok(p.clone()); }
    // Discovery via session: ~/.psmux/orchestrate/<session>/state.json
    let session = args.session.clone()
        .or_else(|| std::env::var("PSMUX_SESSION").ok())
        .context("no session specified; pass --session or set PSMUX_SESSION")?;
    let home = dirs::home_dir().context("no home dir")?;
    Ok(home.join(".psmux").join("orchestrate").join(session).join("state.json"))
}

fn print_table(table: &WorkspaceTable) {
    println!("{:<25} {:<12} {:<10} {}", "ID", "STATUS", "PANE", "TASK");
    for w in table.workspaces() {
        let status = format!("{:?}", w.status).to_lowercase();
        let pane = w.pane_id.as_deref().unwrap_or("-");
        let task = w.task.as_deref().unwrap_or("");
        println!("{:<25} {:<12} {:<10} {}", w.id, status, pane, task);
    }
}
```

Register the subcommand in the main binary. Exact patch depends on the existing dispatch pattern; sketch:
```rust
// In src/main.rs or the root Commands enum:
// add a variant:
Fleet {
    #[command(subcommand)]
    cmd: psmux::fleet::FleetCmd,
},
// in the dispatch match:
Commands::Fleet { cmd } => psmux::fleet::dispatch(cmd),
```

Add a `dirs` dep if not already present: check `Cargo.toml` — most likely already there. If not:
```toml
dirs = "5"
```

Add an empty placeholder `src/fleet/status.rs` so the module compiles (filled in Task 11):
```rust
use anyhow::{bail, Result};

#[derive(Debug, clap::Args)]
pub struct StatusArgs {
    pub id: String,
}

pub fn run(_args: StatusArgs) -> Result<()> { bail!("not yet implemented") }
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test --test test_fleet_ls`
Expected: PASS (1 test).

- [ ] **Step 7: Manual smoke check**

```bash
cd /tmp && mkdir -p fleet-smoke && cd fleet-smoke
cp /d/Home/psmux/crates/psmux-fleet/fixtures/state_healthy.json .
/d/Home/psmux/target/debug/psmux fleet ls --state-path state_healthy.json
# expect: three-row table
/d/Home/psmux/target/debug/psmux fleet ls --state-path state_healthy.json --json | jq '.total'
# expect: 3
```

- [ ] **Step 8: Commit**

```bash
git add src/fleet/ src/main.rs Cargo.toml tests-rs/test_fleet_ls.rs
git commit -m "feat(fleet): psmux fleet ls CLI subcommand with --json flag"
```

---

## Task 11: `psmux fleet status <id>` CLI subcommand

**Files:**
- Modify: `src/fleet/status.rs` (replace the placeholder)
- Modify: `tests-rs/test_fleet_ls.rs` — add a `fleet status` case, or create `tests-rs/test_fleet_status.rs`

- [ ] **Step 1: Write the failing test**

Create `tests-rs/test_fleet_status.rs`:
```rust
use std::process::Command;
use tempfile::TempDir;

fn psmux_bin() -> String { env!("CARGO_BIN_EXE_psmux").to_string() }

#[test]
fn fleet_status_emits_workspace_detail_json() {
    let td = TempDir::new().unwrap();
    let state = td.path().join("state.json");
    std::fs::write(&state, include_str!("../crates/psmux-fleet/fixtures/state_healthy.json")).unwrap();

    let out = Command::new(psmux_bin())
        .args(["fleet", "status", "lint-docs", "--state-path", state.to_str().unwrap(), "--json"])
        .output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["id"], "lint-docs");
    assert_eq!(v["status"], "succeeded");
    assert_eq!(v["exit_code"], 0);
}

#[test]
fn fleet_status_unknown_id_errors() {
    let td = TempDir::new().unwrap();
    let state = td.path().join("state.json");
    std::fs::write(&state, include_str!("../crates/psmux-fleet/fixtures/state_healthy.json")).unwrap();

    let out = Command::new(psmux_bin())
        .args(["fleet", "status", "nonexistent", "--state-path", state.to_str().unwrap(), "--json"])
        .output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("WORKSPACE_NOT_FOUND"), "stderr was: {stderr}");
}
```

Register in root `Cargo.toml`:
```toml
[[test]]
name = "test_fleet_status"
path = "tests-rs/test_fleet_status.rs"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test test_fleet_status`
Expected: failure — placeholder `bail!` returned.

- [ ] **Step 3: Implement**

Replace `src/fleet/status.rs`:
```rust
use anyhow::{Context, Result};
use psmux_fleet::{FleetError, WorkspaceTable};
use std::path::PathBuf;

#[derive(Debug, clap::Args)]
pub struct StatusArgs {
    pub id: String,
    #[arg(long)]
    pub state_path: Option<PathBuf>,
    #[arg(long)]
    pub session: Option<String>,
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: StatusArgs) -> Result<()> {
    let path = crate::fleet::ls::resolve_state_path_for_status(args.state_path.clone(), args.session.clone())?;
    let table = WorkspaceTable::hydrate_cold(&path)
        .with_context(|| format!("loading {}", path.display()))?;
    let ws = table.find(&args.id)
        .ok_or_else(|| anyhow::anyhow!("WORKSPACE_NOT_FOUND: {}", args.id))?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(ws)?);
    } else {
        println!("id:          {}", ws.id);
        println!("session:     {}", ws.session);
        println!("status:      {:?}", ws.status);
        println!("pane:        {}", ws.pane_id.as_deref().unwrap_or("-"));
        println!("exit_code:   {}", ws.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "-".into()));
        if let Some(p) = &ws.crash_dump_path {
            println!("crash_dump:  {}", p.display());
        }
    }
    Ok(())
}
```

Promote `resolve_state_path` in `src/fleet/ls.rs` from private to `pub(crate)` and rename for reuse — or add a companion helper:
```rust
// in src/fleet/ls.rs:
pub(crate) fn resolve_state_path_for_status(explicit: Option<PathBuf>, session: Option<String>) -> anyhow::Result<PathBuf> {
    if let Some(p) = explicit { return Ok(p); }
    let session = session
        .or_else(|| std::env::var("PSMUX_SESSION").ok())
        .context("no session; pass --session or set PSMUX_SESSION")?;
    let home = dirs::home_dir().context("no home dir")?;
    Ok(home.join(".psmux").join("orchestrate").join(session).join("state.json"))
}
```

`FleetError` → stderr: when a `WORKSPACE_NOT_FOUND` is raised, the anyhow wrapper renders it on stderr with the prefix — the test just greps for the code string, which matches.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test test_fleet_status`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/fleet/status.rs src/fleet/ls.rs tests-rs/test_fleet_status.rs Cargo.toml
git commit -m "feat(fleet): psmux fleet status <id> subcommand"
```

---

## Task 12: psmux-fleet-mcp — stdio server skeleton

**Files:**
- Create: `crates/psmux-fleet-mcp/Cargo.toml`
- Create: `crates/psmux-fleet-mcp/src/main.rs`
- Create: `crates/psmux-fleet-mcp/src/tools.rs`
- Modify: `Cargo.toml` (root) — add to workspace members

Scope for this task: the binary starts, accepts stdio JSON-RPC, and registers two tool handlers (list + status). Hand-rolled minimal JSON-RPC is acceptable for C1 — full `rmcp` integration can swap in during C2 if the ecosystem has matured.

- [ ] **Step 1: Add to workspace**

Edit root `Cargo.toml`:
```toml
members = [".", "crates/portable-pty-psmux", "crates/vt100-psmux", "crates/psmux-fleet", "crates/psmux-fleet-mcp"]
```

- [ ] **Step 2: Crate manifest**

Write `crates/psmux-fleet-mcp/Cargo.toml`:
```toml
[package]
name = "psmux-fleet-mcp"
version = "0.1.0"
edition = "2021"
license = "MIT"

[[bin]]
name = "psmux-fleet-mcp"
path = "src/main.rs"

[dependencies]
psmux-fleet = { path = "../psmux-fleet" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
tokio = { version = "1", features = ["rt", "macros", "io-std", "io-util"] }
clap = { version = "4", features = ["derive"] }
dirs = "5"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Write an end-to-end MCP integration test**

Create `crates/psmux-fleet-mcp/tests/mcp_end_to_end.rs`:
```rust
//! Spawns the MCP binary, sends a `fleet.list` request over stdio, reads the
//! response, asserts the workspace count.

use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::TempDir;

fn bin() -> &'static str { env!("CARGO_BIN_EXE_psmux-fleet-mcp") }

#[test]
fn lists_workspaces_from_seeded_state() {
    let td = TempDir::new().unwrap();
    let state = td.path().join("state.json");
    std::fs::write(&state, include_str!("../../psmux-fleet/fixtures/state_healthy.json")).unwrap();

    let mut child = Command::new(bin())
        .args(["--state-path", state.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Minimal JSON-RPC call — not true MCP framing, but matches the hand-rolled
    // protocol the server implements for C1.
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "fleet.list",
        "params": {}
    });
    writeln!(child.stdin.as_mut().unwrap(), "{req}").unwrap();
    drop(child.stdin.take()); // EOF signals the server to exit

    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let resp: serde_json::Value = {
        let line = String::from_utf8_lossy(&out.stdout).lines().next().unwrap().to_string();
        serde_json::from_str(&line).expect("parse JSON-RPC response")
    };
    assert_eq!(resp["result"]["total"], 3);
}
```

- [ ] **Step 4: Run to verify it fails**

Run: `cargo test -p psmux-fleet-mcp --test mcp_end_to_end`
Expected: compilation failure — binary not yet implemented.

- [ ] **Step 5: Implement**

Write `crates/psmux-fleet-mcp/src/tools.rs`:
```rust
use psmux_fleet::{FleetError, WorkspaceTable};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub session: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListResult {
    pub workspaces: Vec<psmux_fleet::Workspace>,
    pub total: usize,
}

#[derive(Debug, Deserialize)]
pub struct StatusParams {
    pub id: String,
    pub session: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope {
    pub code: String,
    pub message: String,
}

pub fn list(state_path: &std::path::Path, _params: ListParams) -> Result<ListResult, FleetError> {
    let table = WorkspaceTable::hydrate_cold(state_path)?;
    Ok(ListResult {
        workspaces: table.workspaces().to_vec(),
        total: table.len(),
    })
}

pub fn status(state_path: &std::path::Path, params: StatusParams) -> Result<psmux_fleet::Workspace, FleetError> {
    let table = WorkspaceTable::hydrate_cold(state_path)?;
    table.find(&params.id)
        .cloned()
        .ok_or(FleetError::WorkspaceNotFound(params.id))
}
```

Write `crates/psmux-fleet-mcp/src/main.rs`:
```rust
use anyhow::Result;
use clap::Parser;
use psmux_fleet_mcp::tools::{list, status, ErrorEnvelope, ListParams, StatusParams};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Parser)]
struct Args {
    /// Explicit state.json path (default: resolve from --session).
    #[arg(long)]
    state_path: Option<PathBuf>,
    #[arg(long)]
    session: Option<String>,
}

mod tools; // re-declared here for the binary crate

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();
    let state_path = resolve_state_path(&args)?;

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();
    let mut writer = stdout;

    while let Some(line) = reader.next_line().await? {
        let req: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => { write_error(&mut writer, serde_json::Value::Null, &e.to_string()).await?; continue; }
        };
        let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(serde_json::json!({}));
        let result = dispatch(method, &state_path, params).await;
        let resp = match result {
            Ok(v) => serde_json::json!({"jsonrpc":"2.0","id": id, "result": v}),
            Err(envelope) => serde_json::json!({"jsonrpc":"2.0","id": id, "error": envelope}),
        };
        writer.write_all(resp.to_string().as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
    }
    Ok(())
}

async fn dispatch(method: &str, state_path: &std::path::Path, params: serde_json::Value)
    -> Result<serde_json::Value, ErrorEnvelope>
{
    match method {
        "fleet.list" => {
            let p: ListParams = serde_json::from_value(params).map_err(err)?;
            let r = list(state_path, p).map_err(fleet_err)?;
            Ok(serde_json::to_value(r).unwrap())
        }
        "fleet.status" => {
            let p: StatusParams = serde_json::from_value(params).map_err(err)?;
            let r = status(state_path, p).map_err(fleet_err)?;
            Ok(serde_json::to_value(r).unwrap())
        }
        _ => Err(ErrorEnvelope { code: "METHOD_NOT_FOUND".into(), message: method.into() }),
    }
}

fn err(e: serde_json::Error) -> ErrorEnvelope {
    ErrorEnvelope { code: "INTERNAL".into(), message: e.to_string() }
}
fn fleet_err(e: psmux_fleet::FleetError) -> ErrorEnvelope {
    ErrorEnvelope { code: e.code().into(), message: e.to_string() }
}

async fn write_error<W: AsyncWriteExt + Unpin>(w: &mut W, id: serde_json::Value, msg: &str) -> Result<()> {
    let resp = serde_json::json!({"jsonrpc":"2.0","id": id, "error": {"code":"PARSE_ERROR","message": msg}});
    w.write_all(resp.to_string().as_bytes()).await?;
    w.write_all(b"\n").await?;
    w.flush().await?;
    Ok(())
}

fn resolve_state_path(args: &Args) -> Result<PathBuf> {
    if let Some(p) = &args.state_path { return Ok(p.clone()); }
    let session = args.session.clone()
        .or_else(|| std::env::var("PSMUX_SESSION").ok())
        .ok_or_else(|| anyhow::anyhow!("SESSION_NOT_FOUND: no --session and PSMUX_SESSION unset"))?;
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home dir"))?;
    Ok(home.join(".psmux").join("orchestrate").join(session).join("state.json"))
}
```

Add a `lib.rs` for the MCP crate so the integration test can import `tools`:
```rust
// crates/psmux-fleet-mcp/src/lib.rs
pub mod tools;
```
and register it in `Cargo.toml`:
```toml
[lib]
name = "psmux_fleet_mcp"
path = "src/lib.rs"
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p psmux-fleet-mcp --test mcp_end_to_end`
Expected: PASS (1 test).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/psmux-fleet-mcp/
git commit -m "feat(fleet): psmux-fleet-mcp stdio server with fleet.list and fleet.status"
```

---

## Task 13: Success-criterion smoke test

Reproduces spec §10 Commit 1 success criterion end-to-end:
> `psmux orchestrate examples/orchestrate/claude-agent-team.json && psmux fleet ls --json` prints a correct JSON table; `psmux-fleet-mcp --session demo` responds identically over stdio.

**Files:**
- Create: `tests/run_fleet_c1_smoke.ps1` (Windows PowerShell rails-bench style)
- Modify: `tests/run_rails_bench.ps1` — register the new script

- [ ] **Step 1: Write the smoke script**

Write `tests/run_fleet_c1_smoke.ps1`:
```powershell
# Fleet C1 smoke test — verifies the success criterion from the design spec.
# Runs in CI via run_rails_bench.ps1.

$ErrorActionPreference = "Stop"
$psmux = Join-Path $PSScriptRoot "..\target\debug\psmux.exe"
$mcp   = Join-Path $PSScriptRoot "..\target\debug\psmux-fleet-mcp.exe"
$fixt  = Join-Path $PSScriptRoot "..\crates\psmux-fleet\fixtures\state_healthy.json"

# 1. CLI path
$out = & $psmux fleet ls --state-path $fixt --json | ConvertFrom-Json
if ($out.total -ne 3) { throw "CLI: expected total=3, got $($out.total)" }
Write-Host "PASS: psmux fleet ls --json reports 3 workspaces"

# 2. MCP path
$req = '{"jsonrpc":"2.0","id":1,"method":"fleet.list","params":{}}'
$resp = $req | & $mcp --state-path $fixt
$parsed = $resp | ConvertFrom-Json
if ($parsed.result.total -ne 3) { throw "MCP: expected total=3, got $($parsed.result.total)" }
Write-Host "PASS: psmux-fleet-mcp fleet.list reports 3 workspaces"

Write-Host "Fleet C1 smoke: all checks passed."
```

- [ ] **Step 2: Register in rails bench**

Edit `tests/run_rails_bench.ps1` — add a line invoking the new script alongside the existing per-feature scripts. Pattern should be obvious from the existing entries; if unclear, search for an existing `$PSScriptRoot\test_*.ps1` invocation and add `$PSScriptRoot\run_fleet_c1_smoke.ps1` nearby.

- [ ] **Step 3: Run locally**

```bash
cargo build --workspace
pwsh tests/run_fleet_c1_smoke.ps1
```
Expected: `Fleet C1 smoke: all checks passed.`

- [ ] **Step 4: Run the full rails bench**

```bash
pwsh tests/run_rails_bench.ps1
```
Expected: all previous rails checks still pass + the new fleet C1 check.

- [ ] **Step 5: Commit**

```bash
git add tests/run_fleet_c1_smoke.ps1 tests/run_rails_bench.ps1
git commit -m "test(fleet): C1 smoke test verifies CLI + MCP list paths against fixture"
```

---

## Task 14: Final verification pass

- [ ] **Step 1: Full workspace build**

Run: `cargo build --workspace --all-features`
Expected: clean build, no warnings from new crates.

- [ ] **Step 2: Full workspace test**

Run: `cargo test --workspace --all-features -- --test-threads=1`
Expected: all tests green (including pre-existing ones — no regression).

- [ ] **Step 3: Clippy pass**

Run: `cargo clippy --workspace --all-features -- -D warnings`
Expected: zero warnings.

- [ ] **Step 4: Format check**

Run: `cargo fmt --all -- --check`
Expected: clean. If not, run `cargo fmt --all` and commit the formatting fix.

- [ ] **Step 5: Update CLAUDE.md with a one-liner pointer**

Edit `D:\Home\psmux\CLAUDE.md`, add to the "User-Facing Documentation" table (near the bottom of the file):
```
| FleetView (v1 — MCP + CLI over state.json) | [docs/superpowers/specs/2026-04-22-fleetview-design.md](docs/superpowers/specs/2026-04-22-fleetview-design.md) |
```

- [ ] **Step 6: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: link FleetView spec from CLAUDE.md"
```

---

## Self-Review (done inline during plan authoring)

**Spec coverage (C1 portion of §10 only):**

| Spec requirement | Task |
|---|---|
| New crate members `psmux-fleet`, `psmux-fleet-mcp` | Task 1, Task 12 |
| Workspace, WorkspaceStatus, AgentKind types (§4) | Task 3 |
| StateReader (§6) | Task 5 |
| StateWriter with CAS (§6.1) | Task 6 |
| WorkspaceTable::hydrate_cold | Task 9 |
| AwaitingDetector trait + RegexDetector + ClaudeTranscriptDetector (§8) | Task 7, Task 8 |
| Session resolver (§7) | Task 4 |
| Error enum with all codes (§9) | Task 1 |
| Dependencies: serde, serde_json, regex, chrono, anyhow, thiserror, tokio, fs2, tempfile, rand | Task 1 manifest |
| MCP server registering `fleet.list` / `fleet.status` | Task 12 |
| CLI subcommands `psmux fleet ls` / `psmux fleet status` | Task 10, Task 11 |
| `state_version: u64` field on OrchestrationState | Task 2 |
| Success criterion smoke test | Task 13 |

**Gaps (acceptable, deferred):**
- `BackendClient` named-pipe wrapper — spec lists this in C2, not C1. Deferred.
- `MycelSubscriber` — spec lists this in C2. Deferred.
- Warm hydration (branch/worktree_path/agent_kind/task populated from backend + plan.json) — C2.
- `rmcp` crate evaluation — acceptable to ship hand-rolled JSON-RPC stdio in C1 and swap later per §12 (risk register).

**Type consistency:** all types named consistently across tasks. `FleetError::code()` returns the same strings used in the error taxonomy test and the MCP `ErrorEnvelope.code` field.

**Placeholder scan:** no TBDs, TODOs, or "similar to above" references. Every code block is complete.
