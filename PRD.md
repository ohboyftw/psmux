# PRD: Tier 0 — Critical Agent Orchestration Fixes

**Goal**: Eliminate send-keys as the programmatic command execution mechanism.
Replace with proper process-level primitives that agent orchestrators (canopy, pi-swarm) can rely on.

**Branch**: `ohboy-builds`
**Date**: 2026-03-22
**Status**: ALL COMPLETE

---

## Phase 1 — Foundation (no cross-dependencies)

### Task 1: `set-option default-shell` — persistent default shell
- [x] Already implemented: `AppState.default_shell`, `apply_set_option`, `get_option_value`, config parsing, pane spawn logic

### Task 2: `new-window -- command args...` — launch command as pane's initial process
- [x] `--` separator handling in `new-window` and `split-window` (connection.rs)
- [x] Everything after `--` is joined as the command string
- [x] Backward-compatible with existing single positional arg parsing

### Task 3: `kill-pane` must remove the pane, not respawn
- [x] KillPane and KillPaneById handlers now immediately remove the window when last pane's process is dead
- [x] No more waiting for reaper tick — window cleanup is synchronous

### Task 4: `list-panes -t %nonexistent` must return non-zero exit code
- [x] `list-windows` client now checks for "can't find" error (matching list-panes)
- [x] FocusPaneTempCheck timeout treated as not-found (was silently proceeding)

---

## Phase 2 — Core Features

### Task 5: `capture-pane` UTF-8 output and ANSI stripping
- [x] Added `--plain` flag: strips all ANSI/VT escape sequences via `strip_ansi_escapes()`
- [x] Handles CSI, OSC, and simple two-byte escapes
- [x] UTF-8 output already ensured via `SetConsoleOutputCP(65001)` in platform.rs

### Task 6: `#{pane_exit_code}` format variable
- [x] Added `exit_code: Option<i32>` to `Pane` struct
- [x] `prune_exited_inner` stores exit code in pane before emitting events
- [x] Format variables `#{pane_exit_code}` and `#{pane_dead_status}` return actual exit code

### Task 7: `psmux exec -t %N "command"` — direct process execution
- [x] New `CtrlReq::Exec` variant with command, shell, and response sender
- [x] Server spawns process in pane's cwd with pane's env
- [x] Shell auto-detection: pwsh `-Command`, bash `-c`, cmd `/C`
- [x] Returns JSON: `{"exit_code":N,"stdout":"...","stderr":"..."}`
- [x] Client forwards exit code as process exit code, 5-min timeout

---

## Phase 3 — Advanced Features

### Task 8: `wait-for -t %N --file <path>` — server-side file watching
- [x] `--file` and `--timeout` flags added to `wait-for`
- [x] Server-side polling at 250ms intervals
- [x] Resolves relative paths against pane's cwd
- [x] Returns "OK" on found, "TIMEOUT" on timeout (client exits 1)
- [x] Default timeout: 1 hour

### Task 9: Mycel event integration for pane lifecycle
- [x] Already implemented behind `--features mycel` flag
- [x] `src/mycel.rs`: MycelBus + publish_pane_event
- [x] Lifecycle hooks: pane/created, pane/died
- [x] Server init at startup with auto-discovery
