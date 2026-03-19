# psmux vs Claude Code v2.1.79 — Compatibility Plan

**Date**: 2026-03-19
**Branch**: ohboy-builds
**Status**: Complete

## Summary

Seven targeted changes across protocol types, dispatcher, server handlers, config defaults, and env propagation. Each task is self-contained with its own tests. The existing `backend_contracts.rs` and `backend_lifecycle.rs` test pattern (mock server + `dispatch_rpc()`) is extended for new methods and edge cases.

## Tasks

### Task 1: DCS passthrough defaults to "on" — [x] DONE
- `src/types.rs:660` — changed default from `"off"` to `"on"`

### Task 2: kill_all RPC method — [x] DONE
- `src/backend/protocol.rs` — added `KillAllParams`, `KillAllResult`
- `src/backend/dispatcher.rs` — added `"kill_all"` route + `handle_kill_all`
- `src/types.rs` — added `BackendKillAll` variant to `CtrlReq`
- `src/server/mod.rs` — server handler that collects agent panes by `@agent` metadata, optional role filter

### Task 3: capture line limiting + clean mode — [x] DONE
- `src/backend/protocol.rs` — added `clean: Option<bool>` to `CaptureParams`
- `src/backend/dispatcher.rs` — passes `p.clean.unwrap_or(false)` through
- `src/server/mod.rs` — implements trailing blank stripping (clean) and last-N-lines limiting

### Task 4: Full AgentMetadata struct + color + frontmatter fields — [x] DONE
- `src/backend/protocol.rs` — added `effort`, `max_turns`, `disallowed_tools` to `AgentMetadata`
- `src/types.rs` — changed `BackendSpawnAgent` metadata from tuple to full `AgentMetadata` struct
- `src/backend/dispatcher.rs` — passes full struct instead of destructured tuple
- `src/server/mod.rs` — stores all metadata fields (`@agent`, `@role`, `@color`, `@effort`, `@max_turns`, `@disallowed_tools`); returns all in `list` response

### Task 5: split_direction for spawn_agent — [x] DONE
- `src/backend/protocol.rs` — added `split_direction: Option<String>` to `SpawnAgentParams`
- `src/types.rs` — added `split_direction` to `BackendSpawnAgent`
- `src/backend/dispatcher.rs` — passes through
- `src/server/mod.rs` — maps `"horizontal"` to `LayoutKind::Horizontal`, default `Vertical`

### Task 6: Non-blocking graceful kill with CTRL_BREAK — [x] DONE
- `src/backend/protocol.rs` — added `grace_ms: Option<u64>` to `KillParams`
- `src/types.rs` — added `grace_ms` to `BackendKillPane`, added `control_tx` to `AppState`
- `src/backend/dispatcher.rs` — passes `p.grace_ms`
- `src/platform.rs` — added `generate_ctrl_break()` unsafe function
- `src/server/mod.rs` — added `get_pane_process_id` helper; non-blocking graceful kill sends CTRL_BREAK then spawns background thread for delayed force-kill

### Task 7: Propagate new Claude Code env vars — [x] DONE
- `src/pane.rs` — propagates 6 new env vars if set in parent:
  - `CLAUDE_CODE_SESSIONEND_HOOKS_TIMEOUT_MS`
  - `CLAUDE_PLUGIN_DATA`
  - `CLAUDE_CODE_PLUGIN_SEED_DIR`
  - `CLAUDE_CODE_DISABLE_TERMINAL_TITLE`
  - `CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS`
  - `ANTHROPIC_CUSTOM_MODEL_OPTION`

### Additional: TMUX env var format — [x] DONE
- `src/pane.rs` — changed from `/tmp/psmux-{pid}/` to `/tmp/tmux-{pid}/` for CC detection compat

## Test Results
- `cargo test` — ALL PASS (31 backend tests + existing tests)
- `cargo clippy -- -D warnings` — CLEAN
- `cargo check` — CLEAN

## Files Modified
| File | Changes |
|------|---------|
| `src/backend/protocol.rs` | New types: `KillAllParams`, `KillAllResult`; extended: `AgentMetadata`, `SpawnAgentParams`, `CaptureParams`, `KillParams` |
| `src/backend/dispatcher.rs` | New handler: `handle_kill_all`; updated: `handle_spawn_agent`, `handle_capture`, `handle_kill` |
| `src/types.rs` | New variant: `BackendKillAll`; new field: `control_tx`; extended: `BackendSpawnAgent`, `BackendKillPane`; changed default: `allow_passthrough` |
| `src/server/mod.rs` | New helper: `get_pane_process_id`; new handler: `BackendKillAll`; updated: `BackendSpawnAgent`, `BackendCapturePane`, `BackendKillPane`, `collect_backend_panes` |
| `src/platform.rs` | New function: `generate_ctrl_break` |
| `src/pane.rs` | TMUX path format, env var propagation |
| `tests/backend_v2179_compat.rs` | NEW — 13 smoke tests |
| `tests/backend_contracts.rs` | Added kill_all + frontmatter contract tests |
| `tests/backend_lifecycle.rs` | Added kill_all lifecycle test + mock handler |
