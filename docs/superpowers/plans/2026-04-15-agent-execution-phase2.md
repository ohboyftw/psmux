# Agent Execution Layer — Phase 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete the agent execution layer with server-side waiting (`wait-for`), full mycel event coverage, and DAG-driven pane orchestration (`orchestrate plan.json`). Eliminates the last client-side polling loops in canopy and agent orchestrators.

**Tech stack:** Rust, Windows Win32 APIs (`WaitForSingleObject`, `ReadDirectoryChangesW`), existing mycel client (crate `mycel-client`), `regex` crate (already a workspace dep), `serde_json`.

**Scoping decisions (from brainstorm):**
- `wait-for --output PATTERN`: match against the **live screen buffer** (vt100-psmux Screen), not scrollback
- Event bus: **mycel-only** (no pluggable sinks). `mycel` is already a cargo feature and `MycelBus` is wired at server startup

---

## Existing Code Map — what already works

| Feature | Status | Location |
|---|---|---|
| `MycelBus` + `publish_pane_event` | Working | `src/mycel.rs` |
| `psmux/pane/created` publish | Working | `src/pane.rs:335, 922` |
| `psmux/pane/died` publish | Working | `src/tree.rs:563` |
| `mycel::init_mycel_bus` at server start | Working | `src/server/mod.rs:631` |
| `context_ready` push event (JSON-RPC) | Working (Phase 1) | `src/backend/dispatcher.rs` |
| `exec_completed` push event (JSON-RPC) | Working (Phase 1) | `src/backend/dispatcher.rs` |
| `Screen::contents()` live-buffer read | Working | `crates/vt100-psmux/src/screen.rs:197` |
| `Pane::process_handle` (for `WaitForSingleObject`) | Present | `src/pane.rs` (ConPTY spawn) |
| `git worktree` invocation | Not needed (done by callers) | — |

---

## File Structure — What Changes

| File | Change | Responsibility |
|---|---|---|
| `src/types.rs` | Add `WaitFor` CtrlReq variant with `WaitCondition` enum | Server request types |
| `src/server/mod.rs` | `WaitFor` handler — dispatches to OS wait primitives | Server event loop |
| `src/server/connection.rs` | `wait-for` CLI subcommand parsing | CLI handler |
| `src/wait_for.rs` (new) | `WaitCondition` executors: exit, file, output, ready | OS-level waiting |
| `src/backend/protocol.rs` | `WaitForParams`, `WaitForResult` | JSON-RPC types |
| `src/backend/dispatcher.rs` | `wait_for` RPC method | JSON-RPC routing |
| `src/mycel.rs` | Add `session_*` and `exec_completed` helpers | Mycel publish helpers |
| `src/server/mod.rs` | Publish `psmux/pane/ready`, `psmux/exec/completed`, `psmux/session/*` | Event instrumentation |
| `src/orchestrate.rs` (new) | `plan.json` loader, DAG resolver, state store | Orchestration |
| `src/main.rs` | `psmux orchestrate <plan.json>` dispatch | CLI wiring |
| `tests-rs/test_wait_for_contracts.rs` (new) | wait-for contract tests | Test coverage |
| `tests-rs/test_mycel_topics.rs` (new) | mycel topic payload contracts | Test coverage |
| `tests/test_orchestrate.ps1` (new) | End-to-end orchestrate flow | Integration |

---

## Feature 4: `psmux wait-for` — server-side blocking wait

### Task 1: `WaitCondition` enum + `wait_for` module skeleton

**Files:**
- Create: `src/wait_for.rs`
- Modify: `src/lib.rs` (add `pub mod wait_for;`)
- Test: `tests-rs/test_wait_for_contracts.rs`

- [x] **Step 1: Write failing contract tests**

```rust
// tests-rs/test_wait_for_contracts.rs
use psmux::wait_for::{WaitCondition, WaitOutcome};

#[test]
fn wait_condition_exit_parses() {
    let c = WaitCondition::parse("exit", Some("1234"), None).unwrap();
    assert!(matches!(c, WaitCondition::Exit { pid: 1234 }));
}

#[test]
fn wait_condition_file_parses() {
    let c = WaitCondition::parse("file", Some("./done.flag"), None).unwrap();
    assert!(matches!(c, WaitCondition::File { path: _ }));
}

#[test]
fn wait_condition_output_requires_regex() {
    let c = WaitCondition::parse("output", Some("^PROMPT_READY$"), None).unwrap();
    assert!(matches!(c, WaitCondition::Output { pattern: _ }));
}

#[test]
fn wait_outcome_is_serializable() {
    let o = WaitOutcome::Success { elapsed_ms: 42 };
    let j = serde_json::to_string(&o).unwrap();
    assert!(j.contains("elapsed_ms"));
}
```

- [x] **Step 2: Define types**

```rust
// src/wait_for.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub enum WaitCondition {
    Exit { pid: u32 },
    File { path: std::path::PathBuf },
    Output { pattern: regex::Regex },
    Ready,  // reuse context_ready signal
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WaitOutcome {
    Success { elapsed_ms: u64 },
    Timeout { elapsed_ms: u64 },
    Error { reason: String },
}

impl WaitCondition {
    pub fn parse(kind: &str, arg: Option<&str>, _opts: Option<&str>) -> Result<Self, String> {
        match kind {
            "exit" => arg.and_then(|s| s.parse().ok())
                .map(|pid| Self::Exit { pid })
                .ok_or_else(|| "exit requires pid".into()),
            "file" => Ok(Self::File { path: arg.ok_or("file requires path")?.into() }),
            "output" => regex::Regex::new(arg.ok_or("output requires pattern")?)
                .map(|r| Self::Output { pattern: r })
                .map_err(|e| format!("invalid regex: {e}")),
            "ready" => Ok(Self::Ready),
            other => Err(format!("unknown wait kind: {other}")),
        }
    }
}
```

### Task 2: `WaitCondition::Exit` via `WaitForSingleObject`

- [x] **Step 1: Failing integration test** — spawn a short-lived process, call wait-for exit, assert `Success { elapsed_ms }`
- [x] **Step 2: Implement** using `OpenProcess` + `WaitForSingleObject` with timeout. Returns `WaitOutcome::Timeout` on `WAIT_TIMEOUT`, `Success` on `WAIT_OBJECT_0`.
- [x] **Step 3: Verify** — test passes, no handle leaks (Drop closes handle).

### Task 3: `WaitCondition::File` via `ReadDirectoryChangesW`

- [x] **Step 1: Failing test** — watch a temp dir, create target file from another thread, assert wake-up within 200ms.
- [x] **Step 2: Implement** using `ReadDirectoryChangesW` async IO on the *parent* directory. Filter events to the target filename. Guard against race: probe filesystem once before arming the watcher (file might already exist).
- [x] **Step 3: Verify** — test passes; pre-existing file case handled.

### Task 4: `WaitCondition::Output` via live screen poll

- [x] **Step 1: Failing test** — prime a vt100 Screen with escape sequences, run wait-for output against a regex, assert match.
- [x] **Step 2: Implement** — poll pane's vt100 `Screen::contents()` every 50ms, run regex. On match, return. Timeout is first-class. **Live buffer, not scrollback** — re-reads current screen each tick.
- [x] **Step 3: Contract test** — regex with multiline flag works; non-matching pattern hits timeout correctly.

### Task 5: `WaitCondition::Ready` piggybacks on `context_ready`

- [x] **Step 1: Test** — subscribe to `context_ready` event, trigger pane readiness, assert wait-for returns.
- [x] **Step 2: Implement** — register a one-shot `oneshot::channel` with the backend event system keyed on `pane_id`. `context_ready` fires → wake channel. Unsubscribe on timeout/success.

### Task 6: CLI + JSON-RPC wiring

- [x] **Step 1: Add `WaitFor` `CtrlReq` variant** (`src/types.rs`) with `{condition, timeout_ms, pane_id}`.
- [x] **Step 2: CLI parser** (`src/server/connection.rs`): `psmux wait-for -t %N --exit PID | --file PATH | --output REGEX | --ready --timeout SECS`.
- [x] **Step 3: Server handler** (`src/server/mod.rs`): dispatches to `wait_for::run(cond, timeout)`, writes `WaitOutcome` as JSON to client stdout, exits with code 0 (success), 1 (timeout), 2 (error).
- [x] **Step 4: JSON-RPC method** (`src/backend/dispatcher.rs`): `wait_for` with same params; returns `WaitOutcome` synchronously.
- [x] **Step 5: End-to-end test** (`tests/test_wait_for.ps1`): spawn pane, write sentinel file after 2s, `psmux wait-for --file` returns < 3s.

### Task 7: Replace canopy's `_wait_sentinel` docs

- [x] **Step 1:** Update `docs/requirements-pi-integration.md` and `README.md` with wait-for examples. No code change in canopy (separate project).

---

## Feature 5: Event bus — complete mycel topic coverage

The bus exists; two topics are already published. Add the missing ones.

### Task 8: Add `psmux/pane/ready` publish

- [x] **Step 1: Failing test** (`tests-rs/test_mycel_topics.rs`) — capture mycel publishes, trigger `context_ready`, assert `psmux/pane/ready` payload `{pane_id, elapsed_ms}`.
- [x] **Step 2: Implement** — in the `context_ready` emission site (same location as backend push event), call `mycel::publish_pane_event("psmux/pane/ready", ...)` under `#[cfg(feature = "mycel")]`.

### Task 9: Add `psmux/exec/completed` publish

- [x] **Step 1: Failing test** — trigger `exec_completed`, assert mycel payload `{pane_id, pid, exit_code, elapsed_ms, command}`.
- [x] **Step 2: Implement** — in dispatcher's `exec_completed` site, co-publish to mycel.

### Task 10: Add `psmux/session/{created,renamed,killed}` publishes

- [x] **Step 1: Failing tests** — `new-session`, `rename-session`, `kill-session` each emit the right topic with `{session_name, client_id}`.
- [x] **Step 2: Implement** — locate the three handlers in `src/server/mod.rs`, add publish calls.

### Task 11: Rename `psmux/pane/died` → `psmux/pane/exited` (spec alignment)

- [x] **Step 1:** Update `src/tree.rs:564` topic string. Grep for consumers — update canopy listener docs if referenced.
- [x] **Step 2: Deprecation shim** (optional, 1 release): publish both topics; remove `/died` next release.

### Task 12: Document topic schema

- [x] Add a `docs/mycel-topics.md` file listing every `psmux/*` topic, its payload schema, and when it fires. Reference from `CLAUDE.md`.

---

## Feature 6: `psmux orchestrate plan.json`

Depends on Feature 4 (`wait-for --exit`) for `depends_on` resolution.

### Task 13: `plan.json` schema + parser

- [ ] **Step 1: Failing tests** — parse minimal plan with two workers and a dependency, validate schema errors.
- [ ] **Step 2: Types** (`src/orchestrate.rs` new file):

```rust
#[derive(Deserialize)]
pub struct Plan {
    pub version: u32,   // must be 1
    pub session: String,
    pub workers: Vec<Worker>,
}

#[derive(Deserialize)]
pub struct Worker {
    pub id: String,
    pub cwd: Option<PathBuf>,
    pub command: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub worktree: Option<WorktreeSpec>,  // Some → create git worktree at cwd
    pub env: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
pub struct WorktreeSpec {
    pub repo: PathBuf,
    pub branch: String,
    pub base: Option<String>,  // defaults to current HEAD
}
```

- [ ] **Step 3:** DAG validation — reject cycles; reject unknown `depends_on` IDs.

### Task 14: State store (`.orchestration/<session>/state.json`)

- [ ] **Step 1: Failing test** — state survives process restart (write, drop handle, re-read).
- [ ] **Step 2: Implement** atomic write (write `state.json.tmp` → rename). Fields: `worker_id → {status: pending|running|succeeded|failed, pane_id, pid, exit_code, started_at, finished_at}`.

### Task 15: Worktree provisioning

- [ ] **Step 1:** For each worker with `worktree`, shell out to `git worktree add <cwd> -b <branch> <base>`. Capture exit code. Abort plan on failure.
- [ ] **Step 2: Teardown** — on `orchestrate --cleanup <session>`, run `git worktree remove <cwd>` for each.

### Task 16: Pane creation + command launch

- [ ] **Step 1:** For each ready worker (no unmet deps), call `new_window -- <command>` in the target session via existing server API. Record `pane_id` + `pid` in state.
- [ ] **Step 2:** Register a `wait-for --exit <pid>` for each running worker. On completion, update state, re-scan for newly unblocked workers, spawn them.

### Task 17: Failure modes

- [ ] **Step 1: Test** — worker exits non-zero → dependents are marked `skipped`, not spawned. Plan exits with aggregated failure code.
- [ ] **Step 2: Test** — orchestrate crash recovery: kill mid-run, re-invoke `orchestrate plan.json`, skips succeeded workers, resumes in-flight ones.

### Task 18: CLI wiring

- [ ] **Step 1:** `psmux orchestrate <plan.json> [--session NAME] [--cleanup]` in `src/main.rs` + dispatch.
- [ ] **Step 2:** Human-readable status output (pane per worker, dep graph) and `--json` machine output.

### Task 19: End-to-end integration test

- [ ] **Step 1:** `tests/test_orchestrate.ps1` — plan with 3 workers (A, B, C; B depends on A; C depends on A+B), all succeed, verify order via exit timestamps.
- [ ] **Step 2:** Failure path test — middle worker fails, downstream skipped.

### Task 20: Docs + example plan

- [ ] **Step 1:** `docs/orchestrate.md` with a canopy-shaped example (feature branch per worker, isolated cwd).
- [ ] **Step 2:** Add row to CLAUDE.md Architecture Notes.

---

## Ordering & Parallelism

- **Feature 4 (Tasks 1–7)**: mostly sequential within a task, but Tasks 2/3/4/5 are independent condition executors — can be parallelized by 4 agents.
- **Feature 5 (Tasks 8–12)**: fully parallelizable; no shared state. Dispatch as a fan-out.
- **Feature 6 (Tasks 13–20)**: Task 13 blocks everything downstream. Tasks 14–16 can overlap once 13 lands. Tasks 17–20 land last.

---

## Verification Gates

Before declaring Phase 2 complete:

- [ ] `cargo fmt --check && cargo clippy -- -D warnings && cargo test --all`
- [ ] `pwsh tests/test_wait_for.ps1` passes
- [ ] `pwsh tests/test_orchestrate.ps1` passes (all paths)
- [ ] Mycel smoke: `mycel sub 'psmux/>'`, run a session, observe `pane/created`, `pane/ready`, `exec/completed`, `pane/exited`, `session/*`
- [ ] Run full garden — no new code-doc drift introduced
- [ ] CLAUDE.md updated with wait-for, mycel topic list, orchestrate

---

---

## Feature 7: Crash diagnostics — minidump hook

**Motivation:** On 2026-04-15 a psmux session exited with zero trace — no `.stackdump`, no panic dump, log cut off mid-activity. Cause undetermined. Phase 2 adds DAG-driven orchestration where silent server death cascades into lost worker state, so we need post-mortem evidence before shipping Feature 6.

### Task 21: Minidump on unhandled exception

**Files:**
- Create: `src/crash.rs`
- Modify: `src/main.rs` (install filter before any other init)
- Modify: `Cargo.toml` (add `minidump-writer = "0.10"` — crates.io Rust minidump writer, no MSVC DbgHelp dep required)

- [ ] **Step 1: Failing test** — `cargo test --test test_crash_handler` spawns a child binary that panics, asserts a `.dmp` file exists in the configured crash dir with non-zero size.
- [ ] **Step 2: Implement**
  - `std::panic::set_hook` → writes minidump + panic message + backtrace to `%LOCALAPPDATA%/psmux/crashes/psmux-{pid}-{unix_ts}.dmp`
  - Win32 `SetUnhandledExceptionFilter` → writes minidump for access violations / stack overflow that bypass panic hook
  - Both hooks log the crash path to `stderr` so Claude Code / orchestrate can surface it
- [ ] **Step 3: CLI surface**
  - `psmux debug crashes list` — list crash dumps newest-first with timestamp + session name
  - `psmux debug crashes show <id>` — print the panic message / exception code
- [ ] **Step 4: Retention** — keep last 20 dumps; prune older on session start.

### Task 22: Orchestrate crash recovery uses crash dir

- [ ] Modify Feature 6 state store (Task 14) to record `crash_dump_path` when a worker's pane dies without an exit code. `orchestrate --resume` surfaces the path to the user.

---

## Out of Scope (explicitly)

- Cross-machine orchestration (`orchestrate` stays single-host; remote work goes via existing `attach-remote`)
- Dynamic plan mutation at runtime — plan is read once at start
- Non-mycel event sinks (NATS, Redis, HTTP webhooks)
- Generic pub/sub subscriptions (`wait-for --topic FOO`) — use mycel sub directly
