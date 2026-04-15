# psmux JSON-RPC Improvements for Canopy Agent Spawning

## Context

Canopy uses psmux's JSON-RPC backend to spawn and monitor agent panes. Live testing revealed that `spawn_agent` **always splits** the active pane, which fails when panes are too small (detached sessions, after multiple splits). Canopy falls back to CLI (`psmux new-window`), defeating the purpose of RPC. The RPC backend needs a way to create new windows, not just splits, and should auto-recover from size errors.

## Changes (all in D:/Home/psmux)

### Phase 1 (must-have): Auto-fallback spawn_agent

**Goal:** `spawn_agent` tries split first, auto-falls back to creating a new window when "pane too small". Zero breaking changes for existing callers.

#### 1a. Add `mode` + `window_name` to SpawnAgentParams (`src/backend/protocol.rs:32`)

```rust
// Add to SpawnAgentParams:
pub mode: Option<String>,        // "split" | "window" | "auto" (default)
pub window_name: Option<String>, // name for new window (window/auto-fallback mode)
```

Add `created_via: String` to `SpawnAgentResult` so callers know what happened.

#### 1b. Add `BackendNewWindow` CtrlReq variant (`src/types.rs`, after line ~1207)

```rust
BackendNewWindow {
    command: Vec<String>,
    cwd: Option<String>,
    env: Option<std::collections::HashMap<String, String>>,
    metadata: Option<crate::backend::protocol::AgentMetadata>,
    shell: Option<String>,
    window_name: Option<String>,
    resp: mpsc::Sender<String>,
},
```

#### 1c. Handle `BackendNewWindow` in server loop (`src/server/mod.rs`, after BackendSpawnAgent handler ~line 5044)

Follow the pattern of `CtrlReq::NewWindow` handler (lines 1038-1106) but with backend additions:
- Join command vec into cmd_str
- Set up cwd, env, stash warm pane (same as BackendSpawnAgent)
- Call `create_window()` instead of `split_active_with_command()`
- Apply metadata to new pane
- Set window name if provided
- Return `%{pane_id}` on success, `ERROR:...` on failure

#### 1d. Mode routing + auto-fallback in dispatcher (`src/backend/dispatcher.rs`, in handle_spawn_agent)

Logic:
- `mode == "window"` -> send `BackendNewWindow` directly
- `mode == "split"` -> send `BackendSpawnAgent` (current behavior), hard-fail on error
- `mode == "auto"` or `None` (default) -> send `BackendSpawnAgent`, if error contains "too small", retry with `BackendNewWindow`

Backward compat: omitting `mode` = `"auto"` = try split, fallback to window. Existing callers get strictly better behavior.

### Phase 2 (should-have): `set_session_size` RPC method

**Goal:** Let canopy set session dimensions before spawning, preventing the root cause of small panes.

#### 2a. Add types (`src/backend/protocol.rs`)

```rust
pub struct SetSessionSizeParams { pub width: u16, pub height: u16 }
pub struct SetSessionSizeResult { pub width: u16, pub height: u16 }
```

#### 2b. Add CtrlReq variant (`src/types.rs`)

```rust
BackendSetSessionSize { width: u16, height: u16, resp: mpsc::Sender<(u16, u16)> },
```

#### 2c. Server handler (`src/server/mod.rs`)

Follow `ClientSize` pattern (line 2016): insert synthetic client dimensions, recompute effective size, `resize_all_panes()`.

#### 2d. Dispatcher (`src/backend/dispatcher.rs`)

Add `"set_session_size" => handle_set_session_size(&req.params, tx)` to dispatch table and implement handler.

### Phase 3 (nice-to-have): `target_window` parameter

Add `pub target_window: Option<usize>` to `SpawnAgentParams` and `BackendSpawnAgent`. Server temporarily switches `active_idx` before splitting. Low priority — auto-fallback to window creation already solves the immediate problem.

## Verification

1. `cargo test` — existing tests pass
2. `cargo build --release` — clean build
3. Manual test sequence:
   ```
   psmux new -s test-rpc -d
   # From canopy Python:
   client = PsmuxBackendClient(pipe)
   await client.connect()
   await client.initialize()
   # spawn with mode=auto (default) — should split first pane
   r1 = await client.spawn_agent(command=["bash"], cwd="/tmp")
   # spawn more until split fails — should auto-fallback to window
   r2 = await client.spawn_agent(command=["bash"], cwd="/tmp")
   r3 = await client.spawn_agent(command=["bash"], cwd="/tmp")
   # explicit window mode
   r4 = await client.spawn_agent(command=["bash"], cwd="/tmp", mode="window")
   ```
4. Check `created_via` field in responses — first spawns say "split", later ones say "window"
5. Run canopy watch against the session — no "pane too small" errors
