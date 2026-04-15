# Agent Execution Layer — Design Spec

**Date**: 2026-04-12
**Branch**: `ohboy-build-agent-first` (off `ohboy-builds`)
**Status**: Phase 1 implementation complete (Tasks 1-10)

## Problem

psmux's agent interface is interactive-first. Creating a pane spawns a shell; commands are injected via `send-keys` (keystroke simulation); output is read via `capture-pane` (screen scraping); completion is detected via polling heuristics (500ms silence, sentinel files). Every agent orchestrator — Claude Code's TeammateTool, Canopy, ECC's dmux-workflows — builds fragile workarounds on this interactive substrate.

**Root cause**: `send-keys` is the wrong abstraction for programmatic agent control. It has no feedback, no error handling, no shell-readiness guarantee, and quoting must match the pane's shell.

## Solution

The Agent Execution Layer makes psmux **programmatic-first** for agent panes while keeping the interactive path unchanged for human users. Six capabilities across three tiers:

```
Tier 1 (execute):    exec, new-window -- command
Tier 2 (observe):    #{pane_exit_code}, enriched push events
Tier 3 (coordinate): wait-for, event bus, orchestrate
```

Each tier builds on the previous. This spec covers the **Phase 1** features (Tier 1 + Tier 2) — the initial 3 features to build.

## Audiences

- **Claude Code TeammateTool users**: #180 (agent teams stability), #191 (capture-pane empty), #172 (can't open team panes). Spawn agents with commands, detect completion.
- **psmux agent orchestration**: Canopy evolution loops, FlowForge pipelines, PaperOrchestra DAGs. Eliminate sentinel files, send-keys fragility, polling loops.

Both audiences suffer from the same root cause. Both benefit from the same fix.

---

## Phase 1 Features (The 3 to Build)

### Feature 1: `psmux exec -t %N "command"`

**Purpose**: Run a command in an existing pane's context (env, cwd) by creating a child process directly — not by injecting keystrokes. Returns the exit code.

**Comparison to send-keys**:

| Aspect | send-keys | exec |
|---|---|---|
| Mechanism | Keystroke injection into PTY | Direct process creation |
| Shell readiness | Must wait for prompt | Irrelevant — bypasses shell |
| Quoting | Caller must match pane's shell syntax | Command parsed by psmux |
| Feedback | None — fire and forget | Exit code, optional stdout capture |
| Shell mismatch | Bash syntax in PowerShell pane = silent fail | Auto shell selection or `--shell` flag |

**CLI interface**:

```bash
# Blocking (default): waits for completion, returns exit code
psmux exec -t %3 "cargo test -- test_name"
echo $?  # 0 = pass, non-zero = fail

# With stdout capture
psmux exec -t %3 --capture "git rev-parse HEAD"

# With timeout
psmux exec -t %3 --timeout 60 "npm run build"

# Shell override
psmux exec -t %3 --shell bash "echo $HOME && ls -la"
```

**JSON-RPC interface** (CustomPaneBackend):

```json
{"method": "exec", "params": {
  "context_id": "%3",
  "command": "cargo test",
  "capture": true,
  "timeout_ms": 60000,
  "shell": "bash"
}}
```

Response:

```json
{"result": {"exit_code": 0, "stdout": "...", "elapsed_ms": 4200}}
```

**Implementation approach**:
- New `CtrlReq::Exec` variant in `src/types.rs` with `pane_id: Option<usize>` for targeting
- Server resolves pane by `pane_id` or falls back to active pane (`src/server/mod.rs`)
- `-t %N` targeting wired through CLI parser in `src/server/connection.rs`
- JSON-RPC method `exec` in `src/backend/dispatcher.rs` (`handle_exec` function)
- `ExecParams`/`ExecResult` types in `src/backend/protocol.rs`
- Process runs outside the PTY — stdout and stderr go back to caller, not to pane's screen
- Pane's interactive shell is undisturbed

**Key files**: `src/types.rs`, `src/server/mod.rs`, `src/server/connection.rs`, `src/backend/dispatcher.rs`, `src/backend/protocol.rs`

**Subsumes**: Tier 2 item #5 (`run-shell` server-side execution), item #6 (`send-keys --wait-ready`).

---

### Feature 2: `#{pane_exit_code}` + Enriched Push Events

One feature with two faces — **pull** (format variables you query) and **push** (events the server sends proactively).

#### Pull: Exit code tracking

When a pane's process exits, psmux records the exit code in pane metadata.

**New format variables**:

| Variable | Value | When |
|---|---|---|
| `#{pane_exit_code}` | Integer (0-255) | After process exits |
| `#{pane_exit_signal}` | Signal name or empty | If killed by signal |
| `#{pane_dead_time}` | Unix timestamp | When process exited |

**Usage**:

```bash
psmux list-panes -F "#{pane_id} #{pane_dead} #{pane_exit_code}"
# %3 1 0       <- exited successfully
# %4 1 1       <- exited with error
# %5 0         <- still running

CODE=$(psmux display-message -t %3 -p "#{pane_exit_code}")
```

**Implementation approach** (implemented):
- `dead_time: Option<u64>` on `Pane` struct — set to Unix epoch ms when process exits via `prune_exited_inner`
- Format variable `pane_dead_time` resolves to real timestamp (ms/1000 for tmux compat), "0" for alive panes
- `ExitedPaneInfo` struct in `src/tree.rs` with `pane_id`, `exit_code`, `elapsed_ms`, `command`
- `spawn_time: std::time::Instant` on Pane for elapsed_ms calculation

**Key files**: `src/types.rs`, `src/tree.rs`, `src/format.rs`, `src/pane.rs`, `src/popup.rs`

**Replaces**: Canopy's `.canopy-done`/`.canopy-failed` sentinel files, capture-pane + regex for success/error strings, any orchestrator's "did this agent finish?" polling.

#### Push: Enriched lifecycle events

The CustomPaneBackend already pushes `context_exited` events over the named pipe. This enriches those and adds new event types.

**Current** (what exists):

```json
{"event": "context_exited", "context_id": "%3"}
```

**Proposed** (3 event types):

```json
{"event": "context_exited", "context_id": "%3",
 "exit_code": 0, "elapsed_ms": 45200, "command": "cargo test"}

{"event": "context_ready", "context_id": "%3",
 "ready_signal": "output_stable", "data_version": 42}

{"event": "exec_completed", "context_id": "%3",
 "exit_code": 1, "stdout_lines": 23, "elapsed_ms": 8400}
```

**Consumers**:
- **Claude Code's TeammateTool**: Already connects to named pipe. Gets success/failure, duration, command info without polling.
- **Canopy / FlowForge**: Subscribe to push events instead of polling `list-panes`. Sub-millisecond completion detection.
- **Health monitor script**: Connects to pipe, consumes events, detects stalls, alerts on failures.

**Implementation approach** (implemented):
- `ContextReadyEvent`/`ContextReadyParams` and `ExecCompletedEvent`/`ExecCompletedParams` in `src/backend/protocol.rs`
- `ContextExitedParams` enriched with `elapsed_ms: Option<u64>` and `command: Option<String>`
- `context_ready` fires from readiness scan in `src/server/mod.rs` — 500ms output stability window, `readiness_notified: bool` prevents duplicates
- `exec_completed` pushed at end of `handle_exec` in `src/backend/dispatcher.rs`
- `context_exited` enrichment uses `ExitedPaneInfo` from `src/tree.rs`

**Key files**: `src/backend/protocol.rs`, `src/backend/dispatcher.rs`, `src/server/mod.rs`, `src/tree.rs`

**Script augmentation**: `scripts/psmux-health-monitor.ps1` — connects to named pipe, consumes push events, implements stall detection and failure alerting with JSON output mode.

---

### Feature 3: `new-window/split-window -- command args`

**Purpose**: Positional arguments after `--` become the pane's initial process instead of the default shell.

**Usage**:

```bash
# Today (fragile):
psmux new-window -d -n build
psmux send-keys -t build "cargo build --release" Enter

# With this feature:
psmux new-window -d -n build -- cargo build --release

# With shell override:
psmux split-window -h --shell bash -- ./run-tests.sh

# Combined with metadata:
psmux new-window -d -n agent -- claude -p "Implement auth"
psmux set-option -t agent -p @role builder
```

**Key behaviors**:
- Pane's process IS the command — when it exits, pane shows exit status (or closes per `remain-on-exit`)
- `#{pane_dead}` means the command finished
- Combined with `#{pane_exit_code}`, you get full lifecycle visibility
- `--shell` flag works: `new-window --shell bash -- ./script.sh`
- JSON-RPC `spawn_agent` already accepts a `command` field — this makes the CLI match the backend

**Implementation approach**: This feature already existed in the codebase prior to this work. The `-- command` separator and `--shell` flag were already wired through `new-window` and `split-window`. Task 8 added end-to-end verification tests confirming it works correctly.

**Key files**: `src/commands.rs`, `src/session.rs`, `src/pane.rs`

---

## Phase 2 Features (Future — Not in Scope)

### Feature 4: `psmux wait-for`

Server-side blocking wait for conditions: `--exit` (process exit via `WaitForSingleObject`), `--file PATH` (file appearance via `ReadDirectoryChangesW`), `--output PATTERN` (regex match against screen buffer), `--ready` (prompt readiness). Replaces all client-side polling loops.

Depends on: Phase 1 exit codes.

### Feature 5: Event bus integration

Optional publication of pane lifecycle events to mycel (`--event-bus localhost:4321`). Topics: `psmux/pane/created`, `psmux/pane/ready`, `psmux/pane/exited`, `psmux/exec/completed`, `psmux/session/*`. Fire-and-forget, non-blocking.

Depends on: Phase 1 enriched push events (same payloads, different transport).

### Feature 6: `psmux orchestrate plan.json`

Reads a plan file, atomically creates git worktrees + panes + metadata for all workers. Supports `depends_on` for dependency resolution (uses wait-for internally). State persisted to `.orchestration/<session>/state.json` for crash recovery.

Depends on: Phase 1 (exec, exit codes, -- command) + Phase 2 (wait-for).

---

## Backlog Items Addressed

| Backlog # | Description | Addressed by |
|---|---|---|
| #22 (Tier 0) | send-keys is wrong for programmatic execution | Feature 1: `exec` |
| #5 (Tier 2) | `run-shell` server-side execution | Subsumed by Feature 1 |
| #6 (Tier 2) | `send-keys --wait-ready` | Subsumed by Feature 1 |
| #27 (Tier 0) | Pane exit code not accessible | Feature 2: `#{pane_exit_code}` |
| #24 (Tier 0) | No way to launch script in new pane directly | Feature 3: `-- command` |
| #21 (Tier 0) | new-window defaults to PowerShell | Partially addressed by Feature 3 `--shell` flag |

## Open Issues Addressed

| Issue | Description | Addressed by |
|---|---|---|
| #180 | Claude Code agent teams stability | Features 1+2+3 eliminate send-keys fragility |
| #191 | capture-pane empty breaking agent teams | Feature 1 `exec --capture` as alternative path |

---

## Testing Strategy

### Unit tests (in `#[cfg(test)]` modules)

- `exec` with valid/invalid pane ID
- `exec` timeout behavior
- `exec` stdout capture
- `exec` shell override (bash vs PowerShell)
- Exit code storage and retrieval
- Format variable resolution for new variables
- Push event serialization for new event types
- `-- command` CLI argument parsing

### Integration tests (in `tests/`)

- `test_exec_basic.rs` — exec a known command, verify exit code
- `test_exec_capture.rs` — exec with --capture, verify stdout content
- `test_exit_code.rs` — spawn pane with command, wait for exit, check `#{pane_exit_code}`
- `test_command_launch.rs` — `new-window -- command`, verify pane runs command and exits
- `test_push_events.rs` — connect to named pipe, spawn + kill pane, verify enriched events

### Contract tests (in `tests-rs/`)

- Extend `test_feature_contracts.rs` with exec and exit code contracts
- JSON-RPC request/response schema validation for new methods and events

### Script tests

- `test_health_monitor.ps1` — verify the health monitor script consumes events correctly
- `test_exec_and_wait.ps1` — verify the exec wrapper handles retries and timeouts
