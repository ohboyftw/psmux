# psmux Requirements for Pi Coding Agent Integration

**Branch:** ohboy-builds  
**Date:** 2026-04-09  
**Status:** Draft  

---

## Background

[psmux](https://github.com/psmux/psmux) is a native Windows terminal multiplexer built in Rust that provides a tmux-compatible CLI alongside a JSON-RPC backend over Windows named pipes. The `pi-teams` extension (v0.9.14) uses terminal adapters to spawn, kill, and inspect agent panes. On Windows without psmux, `pi-teams` falls back to `WindowsAdapter` (limited `wt.exe` CLI — no real kill, synthetic IDs, no alive check). With psmux running, `pi-teams` selects `TmuxAdapter` because psmux sets `$TMUX`, but this path uses `sh -c` command wrapping which fails on Windows.

A new `PsmuxAdapter` has been added to `pi-teams` that detects psmux via `PSMUX_SESSION` env var and uses psmux's tmux-compatible CLI with platform-aware command wrapping (`pwsh`/`cmd` on Windows, `bash` on Unix). To fully unlock the JSON-RPC backend and make psmux a first-class pi citizen, the following changes are needed in psmux.

---

## Requirements

### R1. Add `PSMUX=1` environment variable to all child panes

**Current behavior:** `PSMUX_SESSION` is set in child pane environments, but some code paths set it to literal `"1"` instead of the actual session name. There is no standalone boolean env var for simple detection.

**Requested behavior:** Add `PSMUX=1` to the env vars injected into every child pane. This gives tools a simple boolean check: `if (process.env.PSMUX)` — no string comparison needed.

**Files to change:** `src/pane.rs`

- `set_tmux_env()` line ~1014: Add `builder.env("PSMUX", "1");` after the existing `PSMUX_SESSION` line.
- `build_command()` line ~1255: Add `builder.env("PSMUX", "1");` (where `PSMUX_SESSION` is set to `"1"`).
- `build_default_shell()` line ~1398: Add `builder.env("PSMUX", "1");`.
- `build_for_shell()` line ~1452: Add `builder.env("PSMUX", "1");`.
- `build_raw_command()` line ~1468: Add `builder.env("PSMUX", "1");`.

**Rationale:** Quick boolean check for any tool or adapter, cleaner than checking `PSMUX_SESSION !== undefined`.

---

### R2. Fix `PSMUX_SESSION` value in non-`set_tmux_env()` code paths

**Current behavior:** `set_tmux_env()` (line 1014) correctly sets `PSMUX_SESSION` to the actual session name string (e.g., `"my-session"`). But several other code paths in `pane.rs` (lines 1255, 1281, 1304, 1317, 1398, 1452) set `PSMUX_SESSION` to the literal string `"1"` instead of the real session name.

**Requested behavior:** All code paths that inject `PSMUX_SESSION` must set it to the actual session name, not `"1"`. The session name is available via `app.session_name` or `PSMUX_SESSION_NAME` env var.

**Files to change:** `src/pane.rs`

Pass `session_name` (or resolve from `PSMUX_SESSION_NAME` env var) to all builder functions that currently set `PSMUX_SESSION = "1"`, and use the actual value.

**Rationale:** `PSMUX_SESSION` is meant to be the session name for backend pipe discovery (`~/.psmux/{name}.pipe`). Setting it to `"1"` breaks pipe discovery for panes created outside of `set_tmux_env()`.

---

### R3. Add `PI_PANE_BACKEND_SOCKET` environment variable to child panes

**Current behavior:** Only `CLAUDE_PANE_BACKEND_SOCKET` is injected (line 1018-1019), pointing to the named pipe `\\.\pipe\psmux-claude-backend-{session}`.

**Requested behavior:** Add `PI_PANE_BACKEND_SOCKET` as a second env var with the same value. This allows pi tools to discover the backend pipe without depending on a Claude-specific env var name.

**Files to change:** `src/pane.rs` — `set_tmux_env()` function, after line 1019:

```rust
builder.env("PI_PANE_BACKEND_SOCKET", pipe_path);
```

**Rationale:** Pi is not Claude. Having Pi depend on `CLAUDE_PANE_BACKEND_SOCKET` creates a confusing coupling. `PI_PANE_BACKEND_SOCKET` is the pi-canonical name, while `CLAUDE_PANE_BACKEND_SOCKET` continues working for Claude Code — both point to the same pipe.

---

### R4. Write discovery file on session start (not just on backend listener start)

**Current behavior:** `start_pipe_listener()` in `src/backend/pipe.rs` writes `~/.psmux/{session}.pipe` only when a backend client connects. If the backend listener hasn't started yet, the file doesn't exist.

**Requested behavior:** Write the `~/.psmux/{session}.pipe` file at session creation time (when psmux starts), not deferred to backend listener start. The file should contain the pipe path (e.g., `\\.\pipe\psmux-claude-backend-my-session`).

**Files to change:** `src/pane.rs` or `src/app.rs` — in the session initialization path, write the pipe file early. Keep the existing write in `start_pipe_listener()` as an idempotent update.

**Rationale:** `PsmuxAdapter.resolvePipePath()` tries the discovery file as a fallback when env vars aren't set. If the file doesn't exist yet when pi tries to detect psmux, the adapter falls back to `TmuxAdapter`. Early file creation ensures discovery works from the very first pane.

---

### R5. Make backend pipe name configurable / support `psmux-pi-backend-{session}`

**Current behavior:** The pipe name is hardcoded to `psmux-claude-backend-{session}` in `pipe_path()`.

**Requested behavior:** One of:
- **Option A (recommended):** Keep the single pipe `psmux-claude-backend-{session}` and rely on `PI_PANE_BACKEND_SOCKET` env var + discovery file for pi discovery. No pipe name change needed.
- **Option B:** Add a second named pipe `psmux-pi-backend-{session}` that shares the same dispatcher. Both pipes accept the same JSON-RPC protocol. Clients connect to whichever matches their env var.

Option A is simpler and sufficient because `PsmuxAdapter` already checks `PI_PANE_BACKEND_SOCKET` and `~/.psmux/{session}.pipe`. Recommendation: **Option A**.

---

### R6. Enrich `ContextInfo` in `list` response with `alive`, `cwd`, `title`, and `shell_name`

**Current behavior:** The JSON-RPC `list` method returns `ContextInfo { context_id, metadata }`. No `alive`, `cwd`, `title`, or `shell_name` fields.

**Requested behavior:** Extend `ContextInfo` to include:

```rust
pub struct ContextInfo {
    pub context_id: String,
    pub alive: bool,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub shell_name: Option<String>,
    pub metadata: Option<AgentMetadata>,
}
```

**Files to change:**
- `src/backend/protocol.rs` — add fields to `ContextInfo`
- `src/backend/dispatcher.rs` — `handle_list()` must populate the new fields from pane state

**Rationale:** `PsmuxAdapter.isAlive()` currently falls back to polling `tmux display-message`. With `alive` in the `list` response, the adapter can check pane liveness via a single JSON-RPC call instead of shelling out. `cwd` enables correct working directory tracking. `title` enables pane title display without CLI calls. `shell_name` enables shell-aware command building.

---

### R7. Add `PSMUX_PANE_ID` environment variable to child panes

**Current behavior:** `TMUX_PANE` is set to `%{pane_id}` (e.g., `%0`, `%1`). There's no psmux-specific pane ID.

**Requested behavior:** Add `PSMUX_PANE_ID` env var with the same value as `TMUX_PANE`. This gives pi tools a psmux-branded env var for the current pane identity, independent of tmux convention.

**Files to change:** `src/pane.rs` — `set_tmux_env()`, after line 1014:

```rust
builder.env("PSMUX_PANE_ID", format!("%{}", pane_id));
```

**Rationale:** `TMUX_PANE` is a tmux convention. `PSMUX_PANE_ID` makes it clear this is a psmux-specific value. Downstream tools can use either.

---

## Summary Table

| ID | Change | File(s) | Effort | Impact |
|----|--------|---------|--------|--------|
| R1 | Add `PSMUX=1` env var | `src/pane.rs` | 5 lines | Simple boolean detection |
| R2 | Fix `PSMUX_SESSION` value in all code paths | `src/pane.rs` | ~20 lines | Correct pipe discovery |
| R3 | Add `PI_PANE_BACKEND_SOCKET` env var | `src/pane.rs` | 1 line | Pi-canonical discovery |
| R4 | Write `.pipe` file at session start | `src/pane.rs` or `src/app.rs` | ~10 lines | Early detection guarantee |
| R5 | Keep single pipe (Option A, no change) | None | 0 lines | Simplicity |
| R6 | Enrich `ContextInfo` with alive/cwd/title/shell | `protocol.rs`, `dispatcher.rs` | ~30 lines | Eliminates CLI fallbacks |
| R7 | Add `PSMUX_PANE_ID` env var | `src/pane.rs` | 1 line | Pane identity without tmux convention |

**Total estimated effort:** ~70 lines of Rust changes across 3 files.

---

## Dependency Map

```
PsmuxAdapter (pi-teams)                   psmux (ohboy-builds)
─────────────────────────                 ──────────────────────
PSMUX_SESSION ──env var──→ R2 (fix value)
PSMUX ──env var──→ R1 (add)
PI_PANE_BACKEND_SOCKET ──env var──→ R3 (add)
CLAUDE_PANE_BACKEND_SOCKET ──already exists──→ ✅
~/.psmux/{session}.pipe ──discovery file──→ R4 (write early)
list ──JSON-RPC method──→ R6 (enrich ContextInfo)
PSMUX_PANE_ID ──env var──→ R7 (add)
named pipe path ──no change──→ R5 (Option A)
```

---

## Not Required (Already Working)

- ✅ `TMUX` and `TMUX_PANE` env vars — set correctly by `set_tmux_env()`
- ✅ `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` — set by `set_tmux_env()`
- ✅ `CLAUDE_CODE_NO_FLICKER=1` — set by `set_tmux_env()`
- ✅ `MSYS2_ENV_CONV_EXCL=TMUX` — prevents MSYS2 path mangling
- ✅ Named pipe `\pipe\psmux-claude-backend-{session}` — created and listening
- ✅ JSON-RPC methods: `initialize`, `spawn_agent`, `write`, `capture`, `kill`, `kill_all`, `set_metadata`, `list`, `run_shell`
- ✅ Push event: `context_exited`
- ✅ tmux-compatible CLI: `split-window`, `kill-pane`, `display-message`, `select-pane -T`, `new-window`, etc.
- ✅ `--shell pwsh|bash|cmd` flag in `SpawnAgentParams`
- ✅ `.pipe` discovery file written by `start_pipe_listener()`