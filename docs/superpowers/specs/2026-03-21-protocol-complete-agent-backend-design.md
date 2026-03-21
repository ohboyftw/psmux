# Protocol-Complete Agent Backend — Design Spec

**Date:** 2026-03-21
**Branch:** ohboy-builds
**Status:** Draft
**Goal:** Make CustomPaneBackend production-grade for 5–8 agent Claude Code swarms

---

## Context

psmux's CustomPaneBackend (JSON-RPC over Windows named pipes) provides the spawn/capture/kill
protocol for Claude Code's TeammateTool on Windows. The protocol works in happy-path demos but
has two real-world pain points:

- **Spawn reliability (A):** `spawn_agent` returns before the shell is ready. The caller sends
  commands immediately, they get swallowed by PSReadLine/profile loading.
- **Capture fidelity (C):** `capture` returns whatever is in the VT100 buffer — no freshness
  guarantee. After a `write`, immediate `capture` returns stale pre-command output. Capturing a
  non-existent pane silently returns empty string.

Additionally, Claude Code 2.1.81 introduced `--bare` (skip hooks/LSP/plugins for scripted `-p`
calls) and fixed background agent task output hangs — both directly relevant to swarm reliability.

The agent orchestrator (Canopy) constructs bash commands but psmux always opens the system default
shell (PowerShell), causing silent command failures when shells mismatch.

### Target Scale

5–8 concurrent agents: one leader + 4–7 workers in split panes across 1–2 windows. Pushing the
pane-per-window limit (~6–7 max per window due to minimum pane size constraints).

---

## Feature Set

Eight items: 6 features + 2 targeted fixes.

| # | Feature | Impact | Complexity |
|---|---------|--------|------------|
| 1 | Structured error protocol | Foundation for all other features | Low |
| 2 | Spawn with readiness | Fixes spawn silent failures | Medium |
| 3 | Capture with freshness | Fixes stale capture | Medium |
| 4 | `run_shell` RPC method | Biggest reliability win — bypasses send-keys | Medium-High |
| 5 | Shell selection (`--shell` + `default-shell` + `#{pane_shell}`) | Fixes shell mismatch | Medium |
| 6 | `--bare` aware spawn | Faster agent startup | Low |
| 7 | Validate #143–#146 | Close or fix new issues | Low-Medium |
| 8 | Fix #144 display-panes + #88 Codex scrolling | Targeted UX fixes | Medium |

---

## Feature 1: Structured Error Protocol

### Problem

- `spawn_agent` errors come back as `"ERROR:..."` string prefix in the result field
- `capture` silently returns empty string when the pane doesn't exist
- All RPC errors use generic `-32603` (Internal Server Error) — callers can't distinguish
  "pane too small" from "server channel died"

### Design

Operation-specific error codes in the JSON-RPC server error range (`-32000` to `-32099`):

| Code | Name | When |
|------|------|------|
| `-32001` | `PANE_NOT_FOUND` | capture/kill/write targets a pane_id that doesn't exist |
| `-32002` | `SPAWN_FAILED` | PTY creation or split failed |
| `-32003` | `PANE_TOO_SMALL` | Terminal too small to split another pane |
| `-32004` | `SPAWN_TIMEOUT` | Pane spawned but didn't become ready within deadline |
| `-32005` | `CAPTURE_TIMEOUT` | Waited for fresh output but timed out |
| `-32006` | `SESSION_NOT_FOUND` | Target session doesn't exist |
| `-32007` | `COMMAND_TIMEOUT` | `run_shell` process didn't exit within timeout |
| `-32008` | `COMMAND_FAILED` | `run_shell` process couldn't be spawned |

The `RpcResponse` error object gains a `data` field carrying structured metadata:

```json
{
  "code": -32002,
  "message": "spawn failed: pane too small to split",
  "data": { "pane_count": 7, "window_id": "@0" }
}
```

### Changes

- `protocol.rs` — Add `RpcError` struct with `code: i32`, `message: String`,
  `data: Option<serde_json::Value>`. Add error code constants. Add
  `RpcResponse::error_with_data(id, code, message, data)` constructor alongside the
  existing `RpcResponse::error(id, code, message)` so existing call sites don't need
  modification — new error codes are adopted incrementally.
- `dispatcher.rs` — Replace string-prefix error hacking with typed `RpcError` returns
  across all method handlers. Also add `PANE_NOT_FOUND` checking on `write` method
  (currently fire-and-forget).
- `server/mod.rs` — `BackendSpawnAgent` handler returns structured error info instead
  of `"ERROR:{e}"` strings.

### Protocol Version

Bump `initialize` response `protocol_version` from `"1"` to `"2"`. This gives Canopy a
way to detect whether the connected psmux instance supports the new protocol features.

### Backward Compatibility

Old callers checking for `"ERROR:"` prefix still work since the message field contains
the same text. New callers get machine-readable codes. Callers should check
`protocol_version` in the `initialize` response to determine available features.

---

## Feature 2: Spawn with Readiness

### Problem

`spawn_agent` returns the pane_id as soon as `split_active_with_command()` succeeds, but the
shell inside may still be loading. The existing `pane_ready` infrastructure (500ms silence
heuristic + `data_version` atomic) exists but isn't wired into the spawn path.

### Design

New params on `spawn_agent`:

```json
{
  "method": "spawn_agent",
  "params": {
    "command": ["claude", "--bare", "-p", "do the thing"],
    "wait_ready": true,
    "ready_timeout_ms": 15000
  }
}
```

**`wait_ready`** (bool, default `true`): If true, server polls `pane_ready` using the same
logic as `wait-pane --ready` before returning.

**`ready_timeout_ms`** (u32, default `15000`): How long to wait. Returns `SPAWN_TIMEOUT
(-32004)` with the pane_id in error data if exceeded (so the caller can decide to kill or
wait longer).

### Architecture: Polling in the Dispatcher Thread

**Critical constraint:** The server main loop is single-threaded — it processes `CtrlReq`
messages from an `mpsc::channel`. If the readiness poll loop runs inside the server
handler, it blocks ALL other request processing (captures, writes, key input, rendering,
other spawns).

The existing `wait-pane --ready` in `connection.rs` demonstrates the correct pattern: it
polls from the *client connection thread*, sending `CtrlReq::QueryPaneReady` messages
into the channel and receiving replies. This does NOT block the server loop.

**Server-side flow (two-phase):**

1. `CtrlReq::BackendSpawnAgent` handler in `server/mod.rs` runs
   `split_active_with_command()`, returns the pane_id string immediately (as it does now)
2. Back in `dispatcher.rs::handle_spawn_agent()`: if `wait_ready` is true, the dispatcher
   thread enters a poll loop — sends `CtrlReq::QueryPaneReady` every 200ms, checks
   `data_version` has incremented and `last_output_time` has stabilized (no new output
   for 500ms)
3. Once ready, dispatcher constructs `SpawnResult` and returns. If `ready_timeout_ms`
   exceeded, returns `SPAWN_TIMEOUT (-32004)` with `context_id` in error data.

**Implementation note:** `QueryPaneReady` takes a `usize` pane index, but the backend
protocol uses string `"%N"` format. The dispatcher needs a `parse_pane_id()` helper to
convert `"%5"` → `5usize` for readiness queries.

**Note on `wait_ready: true` default:** This relies on the 500ms silence heuristic. Commands
that produce continuous output (no 500ms pause) will always hit the timeout. Callers spawning
such commands should set `wait_ready: false`.

**Response shape (breaking change):**

```json
{
  "result": {
    "context_id": "%5",
    "ready": true,
    "elapsed_ms": 1200,
    "data_version": 12
  }
}
```

Previously returned just the string `"%5"`. Acceptable: pre-release protocol, only
consumer is Canopy (controlled by the same developer). Uses `context_id` (not `pane_id`)
to match existing protocol naming conventions. Includes initial `data_version` so the
caller can immediately use `since_version` on a subsequent `capture` call.

### Changes

- `protocol.rs` — Add `wait_ready`, `ready_timeout_ms` to `SpawnAgentParams`. Add
  `SpawnResult` struct with `context_id`, `ready`, `elapsed_ms`, `data_version`.
- `dispatcher.rs` — After receiving pane_id from server, run readiness poll loop in
  the dispatcher thread using `CtrlReq::QueryPaneReady`. Construct `SpawnResult`.
- `server/mod.rs` — `BackendSpawnAgent` handler unchanged (returns pane_id string).
- `types.rs` — No new fields needed on `BackendSpawnAgent` variant (readiness polling
  is handled entirely in the dispatcher).

---

## Feature 3: Capture with Freshness

### Problem

`capture` reads whatever is in the VT100 screen buffer at the instant of the call. After
a `write`, immediate `capture` returns stale pre-command output. Capturing a non-existent
pane returns empty string — indistinguishable from empty output.

### Design

New optional params on `capture`:

```json
{
  "method": "capture",
  "params": {
    "context_id": "%5",
    "wait_for_output": true,
    "since_version": 42,
    "timeout_ms": 5000,
    "clean": true,
    "lines": 50
  }
}
```

**`wait_for_output`** (bool, default `false`): Wait until `data_version` increments past
its current value before capturing.

**`since_version`** (u64, optional): Wait until `data_version` exceeds this value. More
precise — the caller tracks version across calls. If omitted and `wait_for_output` is true,
uses the pane's current `data_version` as baseline.

**`timeout_ms`** (u32, default `5000`): How long to wait. Returns `CAPTURE_TIMEOUT (-32005)`
if exceeded, **but includes the current buffer contents in the error data** so the caller
isn't left with nothing.

**Error cases now explicit:**

- Pane not found → `PANE_NOT_FOUND (-32001)` with `{"context_id": "%99"}`
- Timeout → `CAPTURE_TIMEOUT (-32005)` with `{"text": "...", "data_version": 42}` so the
  caller can retry with `since_version`

### Architecture: Polling in the Dispatcher Thread

Same constraint as Feature 2 — the freshness poll loop must NOT run inside the server
main loop handler. The dispatcher thread handles the polling.

**Flow (two-phase):**

1. **Dispatcher** (`dispatcher.rs::handle_capture()`): If `wait_for_output` or
   `since_version` is set, the dispatcher first reads the pane's current `data_version`
   via `CtrlReq::QueryPaneReady`. Then polls every 100ms until `data_version` exceeds
   the baseline, or `timeout_ms` is reached.
2. **Dispatcher** sends `CtrlReq::BackendCapturePane` to get the actual screen contents.
3. **Server handler** (`server/mod.rs`): finds pane, extracts screen — same as today but
   returns `PANE_NOT_FOUND` error instead of silent empty string.
4. **Dispatcher** combines screen text + `data_version` into `CaptureResult`.

On timeout: dispatcher returns `CAPTURE_TIMEOUT (-32005)` but still issues a final
`BackendCapturePane` to include stale contents in the error data.

**Response uses `context_id`** (not `pane_id`) for consistency with existing protocol:

```json
{
  "result": {
    "text": "$ echo hello\nhello\n$",
    "data_version": 47,
    "context_id": "%5"
  }
}
```

The existing `CaptureResult` struct has a `truncated: bool` field that is always set to
`false`. This field is removed — it was never meaningfully used.

### Changes

- `protocol.rs` — Add `wait_for_output`, `since_version`, `timeout_ms` to `CaptureParams`.
  Replace `CaptureResult` struct: remove `truncated`, add `data_version`, `context_id`.
- `dispatcher.rs` — Implement two-phase capture: poll freshness in dispatcher thread via
  `QueryPaneReady`, then issue `BackendCapturePane` for screen extraction.
- `server/mod.rs` — `BackendCapturePane` handler returns `PANE_NOT_FOUND` error instead
  of silent empty string when pane doesn't exist. Screen extraction logic unchanged.

---

## Feature 4: `run_shell` RPC Method

### Problem

To execute a command and get output, the caller must: `write` (send-keys) → sleep/poll →
`capture`. This is fragile — shell might not be ready, PSReadLine garbles input, capture
timing is guesswork. No way to get exit codes.

### Design

New RPC method that executes a command server-side and returns stdout directly:

```json
{
  "method": "run_shell",
  "params": {
    "command": ["git", "status", "--porcelain"],
    "cwd": "/path/to/worktree",
    "timeout_ms": 30000,
    "env": { "GIT_TERMINAL_PROMPT": "0" }
  }
}
```

**Two execution modes:**

**Mode A — Detached (no pane):** Server spawns a child process directly, captures
stdout/stderr, returns it. No PTY, no shell, no pane.

**Mode B — Pane-targeted (`context_id` provided):** Command runs as a detached subprocess
but inherits context from the specified pane.

```json
{
  "method": "run_shell",
  "params": {
    "context_id": "%5",
    "command": ["cat", "output.json"],
    "timeout_ms": 10000
  }
}
```

**Pane cwd/env limitation:** The `Pane` struct does not currently store the working
directory or environment. The shell inside the pane may have `cd`'d since spawn. Two
options were considered:

- **(a) Store spawn-time cwd on Pane** — Cheap but stale if the shell has `cd`'d.
- **(b) Query live cwd via Windows API** — `NtQueryInformationProcess` can read process
  cwd, but it's complex and fragile on Windows.

**Decision: Option (a) + explicit `cwd` override.** Store the spawn-time `cwd` on the
Pane struct. If the caller provides an explicit `cwd` param, that takes priority. This
covers the primary use case (running commands in the worktree the agent was spawned in).
If the caller needs the shell's *current* directory after it has `cd`'d, they should
provide `cwd` explicitly. The spawn-time env is already available in the server process
(env vars are set/restored per-spawn in the existing handler).

**Response:**

```json
{
  "result": {
    "exit_code": 0,
    "stdout": "M  src/main.rs\n?? new_file.txt\n",
    "stderr": "",
    "elapsed_ms": 45
  }
}
```

**Error cases:**

| Code | Name | When |
|------|------|------|
| `-32007` | `COMMAND_TIMEOUT` | Process didn't exit within `timeout_ms` — returns partial stdout, kills process |
| `-32008` | `COMMAND_FAILED` | Binary not found, permission denied, spawn failure |

**Implementation:**

- Uses `std::process::Command` (not PTY) — spawn, wait with timeout, collect output
- If `context_id` provided: look up pane, read its `cwd`, set as working directory
- Env vars merged: server process env + pane env (if context_id) + explicit `env` param
- Timeout: spawn process, wait on thread/channel with timeout. If exceeded, kill process
  and return `COMMAND_TIMEOUT` with partial stdout AND stderr in error data (same
  separation as the success response).
- **Windows process tree limitation:** `Child::kill()` only kills the immediate process,
  not grandchildren. At the 5–8 agent scale, orphaned grandchildren from timed-out
  commands are accepted as a limitation. For commands known to spawn trees (e.g., build
  tools), callers should use generous timeouts.

### Why Not Reuse `write` + `capture`?

- Deterministic — no shell state, PSReadLine, or prompt interference
- Byte-level stdout capture, not VT100 screen scraping
- Exit codes available (capture can never report command success/failure)
- Matches tmux's `run-shell` semantics

### Changes

- `protocol.rs` — Add `RunShellParams`, `RunShellResult`
- `dispatcher.rs` — New `"run_shell"` method handler. Timeout and process management
  run in the dispatcher thread (not the server loop) — spawns
  `std::process::Command`, waits on a thread with timeout.
- `server/mod.rs` — `BackendRunShell` handler: resolve pane spawn-time `cwd` and return
  it to dispatcher. The actual process spawn happens in the dispatcher.
- `types.rs` — Add `BackendRunShell` to `CtrlReq` enum
- `src/pane.rs` / `src/types.rs` — Add `spawn_cwd: Option<PathBuf>` field to `Pane`
  struct, set at spawn time

---

## Feature 5: Shell Selection

### Problem

psmux always spawns the system default shell (PowerShell on Windows). Canopy constructs bash
commands but has no way to request bash. Shell mismatch causes silent command failures.

### Design — Three Pieces

#### 5a. `--shell` flag on `new-window` and `split-window`

```
psmux new-window -t session --shell bash -P -F "#{pane_id}"
psmux split-window -t %5 --shell "C:/Program Files/Git/bin/bash.exe"
```

- Accepts bare name (`bash`, `pwsh`, `cmd`) or absolute path
- Bare names resolved via `PATH` lookup
- Overrides `default-shell` for this specific pane

Also exposed in `spawn_agent` RPC:

```json
{
  "method": "spawn_agent",
  "params": {
    "command": ["claude", "--bare", "-p", "do the thing"],
    "shell": "bash",
    "wait_ready": true
  }
}
```

#### 5b. `default-shell` server option (already implemented)

`default-shell` already exists as a global option — it's handled in `config.rs`,
`server/options.rs`, and stored as `app.default_shell` in `types.rs`. No new code needed.

```
psmux set-option -g default-shell "C:/Program Files/Git/bin/bash.exe"
```

The work here is verification: confirm that the existing `default-shell` option integrates
correctly with the new `--shell` per-pane override. The `--shell` flag takes priority.

#### 5c. `#{pane_shell}` format variable

```
psmux list-panes -F "#{pane_id} #{pane_shell}"
# %0 bash
# %1 pwsh
```

- Reports actual shell binary running in each pane
- Stored in pane metadata at spawn time
- Useful for orchestrators to verify shell selection

**Resolution order:**

1. `--shell` flag (explicit per-pane) — highest priority
2. `default-shell` option (`set-option -g`) — server-wide default
3. System default shell — current behavior, lowest priority

### Changes

- `src/main.rs` — Parse `--shell` arg on `new-window` and `split-window`
- `src/server/mod.rs` — `spawn_pane()`/`split_active_with_command()` accept optional
  shell override, consult `default-shell` option as fallback
- `src/options.rs` — Verify existing `default-shell` works with `--shell` override
- `src/format.rs` — Add `#{pane_shell}` format variable
- `src/pane.rs` — Store shell path in pane metadata at spawn time
- `src/backend/protocol.rs` — Add `shell` field to `SpawnAgentParams`
- `src/backend/dispatcher.rs` — Pass `shell` through to `BackendSpawnAgent`

---

## Feature 6: `--bare` Aware Agent Spawning

### Problem

Claude Code agent startup includes hooks, LSP, plugin sync, and skill directory walks.
This adds 3–5s overhead per agent, widens the PSReadLine race window, and generates noise
in capture output.

### Design

New `bare` flag in `spawn_agent` params:

```json
{
  "method": "spawn_agent",
  "params": {
    "command": ["claude", "-p", "implement the feature"],
    "bare": true,
    "shell": "bash",
    "wait_ready": true
  }
}
```

When `bare: true`: the dispatcher prepends `--bare` to the command array if the first
element contains `claude` (case-insensitive match).

`["claude", "-p", "..."]` becomes `["claude", "--bare", "-p", "..."]`

If the command doesn't look like a Claude invocation, `bare` is ignored — it's a hint,
not a requirement.

**Why a flag instead of putting `--bare` in the command?**

- Orchestrator (Canopy) constructs commands from templates. A first-class param means
  config controls it, not string manipulation.
- Future-proof: if Claude Code renames the flag, psmux updates the mapping in one place.
- Self-documenting: `"bare": true` is clearer than a magic flag in a command array.

### Changes

- `protocol.rs` — Add `bare: Option<bool>` to `SpawnAgentParams`
- `dispatcher.rs` — If `bare` is true and command[0] matches `claude`, inject `--bare`

~15 lines of code.

---

## Feature 7: Validate & Fix #143–#146

### Issues

| Issue | Description | Agent Impact |
|-------|-------------|--------------|
| #146 | `list` commands don't work from inside a psmux session | **High** — agents run `tmux list-panes` to discover peers |
| #145 | `source-file` doesn't work from inside a psmux session | Low |
| #144 | `display-panes` freezes the client | Medium — accidental trigger |
| #143 | Pane numbers remain on screen | Low — cosmetic, related to #144 |

### Approach

1. Reproduce each issue against current `ohboy-builds` binary
2. If already fixed → close issue with note
3. If still broken → fix #146 and #144 (agent-impacting), defer #145/#143 if complex

### Scope Guard

If any fix requires deep architectural changes (e.g., IPC protocol rework for in-session
commands), document the finding and defer to next cycle.

---

## Feature 8: Fix #144 display-panes + #88 Codex CLI Scrolling

### #144 — display-panes freezes the client

**Likely cause:** Blocking overlay mode that doesn't handle input events or cleanup.

**Fix approach:**
- Ensure overlay has a timeout (tmux default: 1 second) and responds to any keypress
- Ensure overlay cleanup repaints underlying panes
- May be a 5-line fix; won't know until reproduced

### #88 — Codex CLI scrolling broken

**Likely cause:** Codex CLI uses alternate screen or mouse protocol. psmux intercepts
scroll events to enter copy mode, eating events that should pass through when the pane
application has requested mouse input.

**Fix approach:**
- Check `parser.screen().mouse_protocol_mode()` before intercepting scroll
- If pane application has captured mouse, pass scroll through instead of entering copy mode
- This matches tmux behavior

**Changes:**
- `src/input.rs` — Check mouse protocol mode before scroll interception
- `crates/vt100-psmux/` — Verify mouse mode tracking

Both fixes scoped as "investigate, fix if straightforward, defer if deep."

---

## Breaking Changes

| Change | Before | After | Migration |
|--------|--------|-------|-----------|
| `spawn_agent` response | `"%5"` (string) | `{"context_id": "%5", "ready": true, "elapsed_ms": 1200, "data_version": 12}` | Update Canopy's spawn result parser |
| `capture` response | `"text..."` (string) | `{"text": "...", "data_version": 47, "context_id": "%5"}` | Update Canopy's capture result parser |
| `capture` result struct | `{text, truncated}` | `{text, data_version, context_id}` | `truncated` removed (was always `false`) |
| `initialize` response | `protocol_version: "1"` | `protocol_version: "2"` | Canopy checks version to detect new features |

All acceptable: pre-release protocol, only consumer is Canopy (same developer).
All response field names use `context_id` (not `pane_id`) for consistency with existing
protocol naming (`CaptureParams`, `WriteParams`, `KillParams`, `ContextInfo`).

---

## Out of Scope

- OSC 133 prompt markers (Phase 2 — replaces 500ms silence heuristic)
- Pi RPC bridge (`pi --mode rpc`)
- Alt key passthrough (#102)
- Mouse selection divergence (#62)
- Popup resize crash (#102)
- Remote tmux control mode hardening
- Warm pool scaling beyond current pool sizes

---

## Dependencies

- Features 2–6 depend on Feature 1 (structured error protocol)
- Feature 6 (`--bare`) depends on Feature 2 (spawn readiness) for full value
- Features 7–8 are independent of 1–6
- Canopy needs a coordinated update for the breaking response shape changes

---

## Future Considerations (Not In Scope)

- **`stdin` support for `run_shell`:** Some commands need piped input. Not critical for
  current agent use cases but worth noting for future protocol versions.
- **Live pane cwd via Windows API:** `NtQueryInformationProcess` could provide the shell's
  current working directory instead of spawn-time cwd. Complex and fragile — deferred
  unless a clear use case emerges.

---

## Success Criteria

1. `spawn_agent` with `wait_ready: true` returns only after the pane shell is ready —
   zero swallowed commands in a 5-agent swarm test
2. `capture` with `wait_for_output: true` after `write` returns post-command output —
   no stale reads in rapid write→capture cycles
3. `run_shell` with `["git", "rev-parse", "--short", "HEAD"]` returns correct commit hash
   with `exit_code: 0` in under 1 second
4. `--shell bash` on `new-window`/`split-window`/`spawn_agent` opens bash, confirmed
   via `#{pane_shell}` format variable
5. All existing tests pass (`cargo test` + swarm E2E suite)
6. #146 (`list` from inside session) works or is documented as deferred with rationale
7. `initialize` returns `protocol_version: "2"` — Canopy can detect new features
