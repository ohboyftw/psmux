# Pi Coding Agent Integration — Design Spec

**Date:** 2026-04-10
**Branch:** ohboy-builds
**Requirements:** `docs/requirements-pi-integration.md`
**Approach:** Direct inline changes (Approach A)

---

## Context

psmux provides a tmux-compatible CLI and a JSON-RPC named pipe backend on Windows. The `pi-teams` extension (v0.9.14) has a new `PsmuxAdapter` that detects psmux via environment variables and uses the backend pipe for pane lifecycle management. This spec covers the psmux-side changes needed to make that adapter work reliably.

## R5 Disposition

R5 (configurable pipe name) is resolved as **Option A — no change**. A single pipe `psmux-claude-backend-{session}` is sufficient; pi discovers it via `PI_PANE_BACKEND_SOCKET` env var or `~/.psmux/{session}.pipe` discovery file.

---

## Section 1: Environment Variable Changes (R1, R2, R3, R7)

**File:** `src/pane.rs`

### R1: Add `PSMUX=1`

Add `builder.env("PSMUX", "1")` in `set_tmux_env()` alongside existing env vars. Also add it in each builder function (`build_command`, `build_default_shell`, `build_raw_command`) for defensive coverage.

### R2: Fix `PSMUX_SESSION` in builder functions

Change builder function signatures to accept `session_name: &str`:

- `build_command(..., session_name: &str)` 
- `build_default_shell(..., session_name: &str)`
- `build_raw_command(..., session_name: &str)`

Replace all `builder.env("PSMUX_SESSION", "1")` with `builder.env("PSMUX_SESSION", session_name)`.

Call sites pass `&app.session_name` (already in scope at all 4 locations: `create_window`, `spawn_warm_pane`, `create_window_raw`, `split_active_with_command`).

**Note:** Currently the "1" placeholder is always overridden by `set_tmux_env()` which runs immediately after each builder. This fix is defensive — ensures correctness even if a future code path uses a builder without calling `set_tmux_env()`.

### R3: Add `PI_PANE_BACKEND_SOCKET`

In `set_tmux_env()`, after the existing `CLAUDE_PANE_BACKEND_SOCKET` line:

```rust
builder.env("PI_PANE_BACKEND_SOCKET", pipe_path);
```

Same pipe path value. Both env vars coexist — Claude Code reads one, Pi reads the other.

### R7: Add `PSMUX_PANE_ID`

In `set_tmux_env()`, after the existing `TMUX_PANE` line:

```rust
builder.env("PSMUX_PANE_ID", format!("%{}", pane_id));
```

Same value as `TMUX_PANE`. Provides psmux-branded pane identity.

---

## Section 2: Discovery File Early Write (R4)

**File:** `src/server/mod.rs` — `run_server()` function

### Current flow

```
Phase 4 (~line 583): Write .port, .key, .version to ~/.psmux/
Phase 7 (~line 646): start_pipe_listener() writes .pipe file
```

### Change

After writing `.port`, `.key`, `.version` files in Phase 4, immediately write the `.pipe` discovery file:

```rust
let pipe_name = crate::backend::pipe::pipe_path(&app.session_name);
let pipe_file = format!("{}\\{}.pipe", dir, app.port_file_base());
let _ = std::fs::write(&pipe_file, &pipe_name);
```

`start_pipe_listener()` retains its existing write as an idempotent update.

### Why

Warm server claiming: when a `__warm__` server is claimed as session "0", the discovery file `~/.psmux/0.pipe` doesn't exist until the pipe listener restarts (if it does). Early write ensures discovery works from session creation.

---

## Section 3: Enrich ContextInfo (R6)

**Files:** `src/backend/protocol.rs`, `src/server/mod.rs`

### ContextInfo struct

```rust
#[derive(Debug, Serialize)]
pub struct ContextInfo {
    pub context_id: String,
    pub alive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_name: Option<String>,
    pub metadata: Option<AgentMetadata>,
}
```

- `alive: bool` — always present, primary field for `PsmuxAdapter.isAlive()`
- `cwd`, `title`, `shell_name` — optional, skip when absent/empty

### List handler

In `src/server/mod.rs`, `collect_backend_panes()` inner function:

```rust
Node::Leaf(p) => {
    let meta = AgentMetadata::from_metadata_map(&p.metadata);
    out.push(ContextInfo {
        context_id: format!("%{}", p.id),
        alive: !p.dead,
        cwd: p.spawn_cwd.as_ref().map(|p| p.to_string_lossy().into_owned()),
        title: if p.title.is_empty() { None } else { Some(p.title.clone()) },
        shell_name: p.shell_name.clone(),
        metadata: meta,
    });
}
```

All fields sourced from existing pane state — no new tracking needed.

---

## Files Changed

| File | Changes |
|------|---------|
| `src/pane.rs` | R1, R2, R3, R7: env var additions, builder signature changes |
| `src/server/mod.rs` | R4: early .pipe write; R6: list handler enrichment |
| `src/backend/protocol.rs` | R6: ContextInfo struct extension |

## Estimated Diff

~70 lines of Rust across 3 files. No new dependencies. No breaking changes to existing JSON-RPC consumers (new fields are additive).

## Testing

- Existing `cargo test` suite validates pane creation, env injection, backend protocol
- Manual verification: start session, inspect child pane env vars, call JSON-RPC `list`
- Confirm `PsmuxAdapter` in pi-teams detects psmux via `PSMUX=1` and reads enriched `list` response
