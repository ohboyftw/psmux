# Design Spec: "iTerm2 for Windows" + CustomPaneBackend

**Date:** 2026-03-18
**Branch:** ohboy-builds
**Status:** Approved design, pending implementation plan
**Authors:** Aravind + Claude Opus 4.6

## 1. Overview

Two independent features built in parallel on psmux's ohboy-builds branch:

- **Feature A — tmux Control Mode Client ("iTerm2 for Windows")**: psmux connects to a remote Linux tmux server via SSH + `tmux -CC`, parses the control mode protocol, and renders remote panes in psmux's native UI. Phase 1 is remote-only sessions (no hybrid local+remote panes in the same window).

- **Feature B — CustomPaneBackend JSON-RPC Server**: psmux exposes a JSON-RPC 2.0 endpoint over a Windows named pipe (`\\.\pipe\psmux-claude-backend-{session}`), implementing the 7-operation protocol from [claude-code#26572](https://github.com/anthropics/claude-code/issues/26572). This makes psmux the official Claude Code agent teams backend on Windows.

- **Feature C — DCS Passthrough**: Support `set -g allow-passthrough on` so Claude Code 2.1.78's tmux passthrough notifications escape psmux to the host terminal.

Canopy integration is a follow-on phase designed alongside these features but built separately.

## 2. Motivation

### Competitive Position

| Tool | Local mux | Remote tmux integration | Windows native | Agent orchestration |
|------|-----------|------------------------|----------------|---------------------|
| iTerm2 | Yes | Yes (tmux -CC) | No (macOS only) | No |
| Windows Terminal | Yes (tabs) | No (requested 2020, never built) | Yes | No |
| WezTerm | Yes | SSH domains (own protocol) | Yes | No (maintainer rejected AI) |
| Zellij | Yes | No | No (Linux/macOS only) | Plugin only |
| **psmux (proposed)** | **Yes** | **tmux -CC** | **Yes** | **Yes (ohboy-builds + CustomPaneBackend)** |

psmux would be the only tool combining all four columns.

### Why These Features

1. **Remote multiplexing** is the #1 gap holding psmux back. Every competitor works over SSH; psmux is local-only. The tmux control mode approach requires zero install on the remote host — tmux is already there.

2. **CustomPaneBackend** eliminates the race condition in Claude Code's current tmux CLI shim (claude-code#23615: `split-window` + `send-keys` are separate stateless subprocesses, ~50% corruption rate on 4+ agents). A persistent JSON-RPC connection serializes all operations.

3. **DCS Passthrough** is required by Claude Code 2.1.78, which emits terminal notifications wrapped in `\x1bPtmux;...\x1b\\`. Without this, psmux silently drops all notifications.

## 3. Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Build order | Both features in parallel | Architecturally independent — one is a new client mode, the other is a new server mode |
| CustomPaneBackend transport | Pre-running socket server (not spawn-on-demand) | psmux already runs as a persistent server with TCP listeners; adding a named pipe endpoint is natural |
| Remote mux approach | tmux control mode (-CC), not remote psmux | Zero install on remote; tmux already on every Linux box; documented protocol; sessions survive SSH drops |
| Remote session scope | Remote-only first, hybrid (mixed local+remote) in Phase 2 | Clean MVP; hybrid requires unifying two pane backends in one layout tree |
| Named pipe vs TCP for backend | Named pipe (`\\.\pipe\psmux-claude-backend`) | Native Windows IPC; inherits OS-level user auth; matches `CLAUDE_PANE_BACKEND_SOCKET` env var semantics |

### Local + Remote UX Without Hybrid

Users run separate sessions and switch between them:

```
Terminal 1:  psmux new-session -s local        # Windows panes (ConPTY)
Terminal 2:  psmux attach-remote user@vm       # Linux panes (tmux -CC)
```

Switch with `Ctrl+b (` / `Ctrl+b )` (previous/next session) or `Ctrl+b s` (session chooser). For agent teams, two patterns work:

- **All-remote**: Leader + agents run on Linux VM via tmux. User watches from Windows through psmux.
- **Local leader, remote workers**: Leader spawns local agents via CustomPaneBackend, monitors remote agents via SSH + `capture-pane`.

## 4. Architecture — Feature A: tmux Control Mode Client

### New Module: `src/remote/` (~800-1000 LOC across submodules)

```
┌─────────────────────────────────────────────────────────┐
│  src/remote.rs                                          │
│                                                         │
│  ┌─────────────┐    ┌──────────────┐    ┌────────────┐ │
│  │ SshTransport │───>│ ControlMode  │───>│ RemotePane │ │
│  │              │    │ Parser       │    │ Manager    │ │
│  │ - spawn ssh  │    │              │    │            │ │
│  │ - stdin/out  │    │ - %output    │    │ - pane map │ │
│  │ - reconnect  │    │ - %window-*  │    │ - resize   │ │
│  └─────────────┘    │ - %session-* │    │ - focus    │ │
│                     │ - %begin/end │    └────────────┘ │
│                     │ - %pause     │                    │
│                     └──────────────┘                    │
└────────────────────────┬────────────────────────────────┘
                         │
                         v
        ┌────────────────────────────────┐
        │  Existing psmux infrastructure │
        │  (reused unchanged)            │
        │                                │
        │  - vt100::Parser (per pane)    │
        │  - Layout engine (pane tree)   │
        │  - Status bar + format vars    │
        │  - Copy mode (vim keybindings) │
        │  - Client renderer (ratatui)   │
        └────────────────────────────────┘
```

### Component 1: SshTransport

Manages the SSH child process.

- Spawns `ssh user@host tmux -CC attach -t {session}` (or `new-session`)
- Owns stdin (write commands) and stdout (read notifications)
- Handles SSH connection drops with reconnect + exponential backoff
- Sends `refresh-client -C {w}x{h}` on terminal resize

### Component 2: ControlModeParser

Line-oriented state machine parsing the tmux control mode wire protocol.

**State machine:** `Idle -> InBlock -> Idle`

**Parses:**
- `%begin {timestamp} {command_number} {flags}` ... `%end` / `%error` response blocks (space-separated fields; `command_number` is monotonically increasing for correlation; `flags` is numeric, typically 0)
- 18 async notification types (see Contract 1 below), including `%layout-change`
- Response blocks can be multi-line (e.g., `list-windows` returns one line per window)
- Octal escape decoding (tmux encodes chars < ASCII 32 and `\` as octal)
- Flow control (`%pause` / `%continue` / `%extended-output`)

**Wire protocol reference:** [tmux Control Mode wiki](https://github.com/tmux/tmux/wiki/Control-Mode)

### Component 3: RemotePaneManager

Bridges parsed notifications to psmux's rendering pipeline.

- Maintains `HashMap<String, vt100::Parser>` — one VT100 parser per remote pane
- `%output %N <data>` feeds decoded bytes to the correct pane's parser
- `%window-add @N` / `%window-close @N` updates psmux's window list
- Keystroke forwarding: user input → `send-keys -t %{active_pane} {key}` over SSH stdin
- Prefix key (`Ctrl+b`) handled locally — never forwarded to remote tmux

### Data Flow

```
User keystroke
  -> psmux client intercepts
  -> if prefix: handle locally (split, resize, copy mode, session switch)
  -> else: SSH stdin -> "send-keys -t %{pane} {escaped_key}\n"
  -> remote tmux processes key
  -> remote app produces output
  -> tmux sends "%output %{pane} {octal_encoded_data}\n" on SSH stdout
  -> ControlModeParser decodes
  -> RemotePaneManager feeds to vt100::Parser
  -> psmux renders frame (same path as local panes)
```

### Integration with Existing Client

- `src/client.rs` currently has `run_remote()` for local persistent TCP connections
- New function: `run_remote_tmux(ssh_target, session_name)` — same rendering loop, reads from ControlModeParser instead of TCP frame channel
- Reuses: `render_layout()`, `handle_copy_mode()`, `handle_prefix()`, `draw_status_bar()`
- The client doesn't know if panes are local or remote — it gets VT100 cell grids either way

### CLI Additions

```
psmux attach-remote <ssh-target> [-t session] [--ssh-opts "..."]
psmux new-session-remote <ssh-target> -s <name> [-n window-name]
psmux list-sessions-remote <ssh-target>
psmux detach-remote   # also Ctrl+b d (handled locally)
```

## 5. Architecture — Feature B: CustomPaneBackend JSON-RPC Server

### New Module: `src/backend.rs` (~300 LOC)

```
┌───────────────────────────────────────────────────────────┐
│  Claude Code process                                      │
│                                                           │
│  CLAUDE_PANE_BACKEND_SOCKET=\\.\pipe\psmux-claude-backend │
│  Sends JSON-RPC 2.0 over named pipe (NDJSON)              │
└──────────────┬────────────────────────────────────────────┘
               | Named Pipe (Windows)
               v
┌───────────────────────────────────────────────────────────┐
│  src/backend.rs                                           │
│                                                           │
│  ┌─────────────────┐    ┌──────────────────┐              │
│  │ PipeListener     │───>│ RpcDispatcher    │              │
│  │                  │    │                  │              │
│  │ - accept conn    │    │ - parse JSON-RPC │              │
│  │ - per-client     │    │ - route method   │              │
│  │   thread         │    │ - send response  │              │
│  └─────────────────┘    │ - push events    │              │
│                         └────────┬─────────┘              │
│                                  │                        │
│                    ┌─────────────v──────────────┐         │
│                    │ Method -> CtrlReq Mapping   │         │
│                    │                             │         │
│                    │ initialize    -> self pane  │         │
│                    │ spawn_agent   -> SplitWindow│         │
│                    │ write         -> SendText   │         │
│                    │ capture       -> CapturePane│         │
│                    │ kill          -> KillPane   │         │
│                    │ list          -> ListPanes  │         │
│                    │ context_exited<- wait-pane  │         │
│                    └─────────────┬──────────────┘         │
│                                  | mpsc::Sender<CtrlReq>  │
│                                  v                        │
│                    ┌───────────────────────────┐          │
│                    │ Existing server event loop │          │
│                    │ (src/server/mod.rs)        │          │
│                    └───────────────────────────┘          │
└───────────────────────────────────────────────────────────┘
```

### The 7 Operations

| JSON-RPC Method | psmux CtrlReq | Notes |
|----------------|---------------|-------|
| `initialize` | Read `app.active_pane_id` | Returns `self_context_id`, protocol version, capabilities |
| `spawn_agent` | `SplitWindow` + `SetPaneOption(@agent)` | Takes `argv[]` not shell string. Spawns process directly — no `send-keys`. Returns `context_id = %{pane_id}` |
| `write` | `SendText` with target | Base64 decode, write to pane PTY stdin |
| `capture` | `CapturePane` with `--clean` | Returns captured text, optional line count |
| `kill` | `KillPane` with target | Kills pane, triggers `context_exited` |
| `list` | `ListPanes` with `--json` | Returns array of `{context_id, metadata}` |
| `context_exited` (push) | Pane death detection | Pushes when child process exits. Uses existing `pane.dead` + `wait-pane` infrastructure |

### Key Design Points

**`spawn_agent` takes `argv[]`, not shell strings.** This eliminates shell interpolation and quoting bugs. psmux spawns the child process through `portable-pty-psmux`'s `CommandBuilder` (which wraps ConPTY + `CreateProcessW`), ensuring the agent gets a proper PTY and detects interactive mode correctly. No `cmd.exe /c` shell wrapping. Windows `CreateProcess` argument quoting rules are handled by `CommandBuilder`.

**Persistent connection serializes all operations.** This fixes the race condition from claude-code#23615 where concurrent `split-window` + `send-keys` subprocess calls corrupt state at 4+ agents.

**Auth is implicit.** Named pipes inherit Windows security descriptors — only the same user can connect. No session key exchange needed.

**Pipe path discovery:** Session-scoped only. Pipe path `\\.\pipe\psmux-claude-backend-{session}` is written to `~/.psmux/{session}.pipe` alongside `.port` and `.key` files. No singleton well-known path — multiple psmux sessions must each have their own pipe to avoid collisions. Claude Code discovers the pipe via `CLAUDE_PANE_BACKEND_SOCKET` env var (set by psmux at session creation) or by reading the `.pipe` file.

### Lifecycle

1. psmux server starts named pipe listener alongside TCP listener (in `run_server()`)
2. Claude Code sets `CLAUDE_PANE_BACKEND_SOCKET=\\.\pipe\psmux-claude-backend`
3. Claude Code connects, sends `initialize` -> gets `self_context_id`
4. Claude Code sends `spawn_agent` -> psmux creates pane, returns `context_id`
5. Claude Code sends `write` -> psmux forwards to pane stdin
6. When agent pane exits -> psmux pushes `context_exited` notification
7. Claude Code sends `kill` on cleanup

### Event Push Mechanism

- Each RPC client connection gets an `mpsc::Sender<String>` for push events
- When a pane dies (detected in server main loop via `child.try_wait()`), server sends `context_exited` to all registered RPC clients
- Reuses the same pane death detection that `wait-pane` already uses

### NDJSON Framing

The named pipe is **full-duplex** with interleaved requests, responses, and push notifications. Each JSON message is a single line (newline-delimited). The client must parse each line independently — a `context_exited` push event can arrive between a request and its response. The server uses separate read and write threads per client connection:
- Read thread: receives requests, dispatches to `CtrlReq` channel, waits for response, writes response line
- Write thread: receives push events from `mpsc::Receiver`, writes notification lines
- Both threads write to the same pipe; writes are serialized via a `Mutex<PipeWriter>` to prevent interleaving partial lines

## 6. Architecture — Feature C: DCS Passthrough

Required by Claude Code 2.1.78's tmux passthrough notifications.

### How tmux Passthrough Works

Applications inside tmux wrap escape sequences in a DCS envelope:
```
\x1bPtmux;\x1b<inner_escape_sequence>\x1b\\
```

tmux strips the wrapper and forwards the inner sequence to the host terminal when `allow-passthrough on`.

### Current psmux State

- `allow-passthrough` config option exists in `AppState` but is a **no-op**
- `vt100-psmux` fork's `Perform` trait implementation has **no `hook()`/`put()`/`unhook()` DCS handlers** — all DCS sequences are silently dropped
- No code writes raw escape sequences from child panes to host terminal stdout (except DECSCUSR cursor shape and OSC 52 clipboard)

### Implementation

```
Child process (Claude Code)
  sends: \x1bPtmux;\x1b<seq>\x1b\\
         |
         v (PTY output)
vt100-psmux parser (src/crates/vt100-psmux/src/perform.rs)
  NEW: implement hook()/put()/unhook() for DCS
  detect "tmux;" prefix -> buffer inner sequence
         |
         v (DCS callback)
Passthrough queue (src/types.rs)
  per-pane Arc<Mutex<Vec<Vec<u8>>>>
  gated on: allow-passthrough setting + active pane check
         |
         v (drain queue)
Client render loop (src/client.rs)
  after rendering frame, write raw bytes to stdout
  bypasses ratatui — goes directly to host terminal
```

### Config

```
set -g allow-passthrough off   # default, safe — silently consume
set -g allow-passthrough on    # forward from active pane only
set -g allow-passthrough all   # forward from any pane
```

### Files Changed

1. `crates/vt100-psmux/src/perform.rs` — add DCS `hook()`/`put()`/`unhook()` methods
2. `crates/vt100-psmux/src/callbacks.rs` — add `dcs_passthrough(&mut self, data: &[u8])` callback
3. `src/pane.rs` — passthrough queue per pane, fed by DCS callback
4. `src/client.rs` — drain active pane's queue after frame render, write raw to stdout
5. `src/types.rs` — `PassthroughQueue` struct (max depth 64 entries, oldest-discard to prevent unbounded growth from misbehaving child processes)

**Estimated size:** ~80-100 LOC across these files. The `vt100-psmux` fork is the main unknown — need to verify how the `vte` crate's `Perform` trait exposes DCS data.

## 7. Claude Code 2.1.78 Impact Analysis

Full analysis performed 2026-03-18. Items organized by priority:

### Critical

| Item | Impact | Action |
|------|--------|--------|
| **tmux passthrough notifications** | Claude Code 2.1.78 emits DCS passthrough; psmux drops them silently | Implement Feature C (DCS Passthrough) |
| **CustomPaneBackend alignment** | Proposal (#26572) still open, 14 thumbs-up | Implement Feature B; track upstream for protocol changes |

### Important

| Item | Impact | Action |
|------|--------|--------|
| **StopFailure hook** | Canopy's Stop hook won't fire on rate-limited agents; agents appear "hung" | Canopy: add `StopFailure` handler writing `.canopy-failed` |
| **Plugin-shipped agent frontmatter** | `.claude/agents/*.md` can now use `effort`, `maxTurns`, `disallowedTools` | Update agent definitions (psmux-reviewer: disallow Edit/Write/Bash) |
| **Worktree skill/hook loading fix** | Skills now load correctly from git worktrees | No action needed; fixes previous reliability gap for agent teams |
| **Large session resume fix** | Sessions >5MB with subagents now resume correctly | No action needed; removes operational pain for agent team sessions |
| **Infinite loop fix (Stop hooks)** | API errors no longer trigger infinite hook loops | No action needed; Sisyphus/Canopy stop hooks are now safer |

### Nice-to-Have

| Item | Impact | Action |
|------|--------|--------|
| Line-by-line streaming | Output arrives in line chunks; affects `capture-pane --clean` timing | No action; UX improvement works automatically |
| `ANTHROPIC_CUSTOM_MODEL_OPTION` | Per-pane model selection possible via env var | Could add as pane option; low priority |
| Denied MCP tools fix | Properly filtered before model sees them | No action; fixes waste in agent team panes |
| `allowWrite` absolute path fix | Sandbox works correctly for worktree paths | No action |

### No Impact

| Item | Reason |
|------|--------|
| PATH lookup for Homebrew | macOS-only; psmux is Windows-only |
| Missing sandbox dependency warning | Shows inside psmux pane; no psmux changes |
| Protected dirs in bypassPermissions | Security fix internal to Claude Code |

## 8. Canopy Integration Phase

Designed alongside psmux features but built separately. All changes are additive and gated behind config flags.

### Phase 1: CustomPaneBackend Client

**New module: `canopy/psmux_rpc.py`** (~120 LOC)

`PsmuxBackendClient` class connects to psmux's named pipe and exposes async methods: `initialize()`, `spawn_agent()`, `capture()`, `kill()`, `list_contexts()`, `events()`.

**Changes to existing modules:**

| Module | Change | Gated by |
|--------|--------|----------|
| `config.py` | Add `psmux.backend_socket: Optional[str]` | Config flag |
| `spawner.py` | Add `_spawn_via_backend()` path | `if config.psmux.backend_socket` |
| `monitor.py` | Subscribe to `context_exited` events as fast-path | Backend available |
| `gc.py` | Use `list_contexts()` RPC instead of subprocess | Backend available |
| `loop.py` | Init backend client at startup if socket exists | Socket file exists |

**Fallback:** If named pipe doesn't exist or connection fails, Canopy falls back to CLI subprocess mode. Zero breakage.

### Phase 2: Remote Execution

**Config additions:**

```python
class RemoteHost(BaseModel):
    name: str                       # "build-vm"
    ssh: str                        # "user@10.0.0.5"
    session: str = "canopy"         # remote tmux session name
    worktree_root: str = "/tmp/canopy-worktrees"
    capabilities: list[str] = []    # ["docker", "gpu"]
```

**Module changes:**

| Module | Change |
|--------|--------|
| `config.py` | Add `remote: RemoteConfig` section |
| `models.py` | Add `ExecutionTarget` enum (LOCAL/REMOTE), `remote_host` field on Task |
| `router.py` | Add `_pick_execution_target()` — route by `#remote` tag or keyword matching |
| `spawner.py` | Add `_spawn_remote()` — SSH worktree creation + psmux remote pane |
| `monitor.py` | Add `_check_remote_sentinels()` — batched SSH sentinel file checks |
| `gc.py` | Add remote worktree cleanup |

### StopFailure Hook (from 2.1.78)

Canopy's existing Stop hook only fires on normal completion. Rate-limited agents that end with `StopFailure` leave no sentinel file, making agents appear "hung."

**Fix:** Add `StopFailure` handler in worktree's `.claude/settings.local.json`:

```json
{
  "hooks": {
    "Stop": [{"command": "touch .canopy-done"}],
    "StopFailure": [{"command": "echo 'API error: rate limit or auth failure' > .canopy-failed"}]
  }
}
```

## 9. Boundary Contracts

### Contract 1: Control Mode Wire Protocol (SSH stdout -> ControlModeParser)

**Parsed message type:**

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ControlModeMessage {
    Response {
        timestamp: u64,
        command_id: u64,
        flags: u32,
        body: Vec<String>, // multi-line responses (e.g., list-windows returns one line per window)
        success: bool,     // true = %end, false = %error
    },
    Output { pane_id: String, data: Vec<u8> },
    ExtendedOutput { pane_id: String, lag_ms: u64, data: Vec<u8> },
    WindowAdd { window_id: String },
    WindowClose { window_id: String },
    WindowRenamed { window_id: String, new_name: String },
    UnlinkedWindowAdd { window_id: String },
    UnlinkedWindowClose { window_id: String },
    UnlinkedWindowRenamed { window_id: String, new_name: String },
    SessionChanged { session_id: String, session_name: String },
    SessionRenamed { session_id: String, new_name: String },
    SessionsChanged,
    SessionWindowChanged { session_id: String, window_id: String },
    ClientSessionChanged { client: String, session_id: String, session_name: String },
    WindowPaneChanged { window_id: String, pane_id: String },
    PaneModeChanged { pane_id: String },
    LayoutChange { window_id: String, layout_string: String },
    Pause { pane_id: String },
    Continue { pane_id: String },
    SubscriptionChanged { name: String, value: String },
    Exit { reason: Option<String> },
}
```

**Design notes:**
- psmux will NOT enable `extended-output` mode in v1 (to keep parser simpler). Standard `%output` is sufficient. `%extended-output` support is deferred to a future version if flow control becomes necessary for high-latency remote connections.
- `%layout-change @{window_id} {layout_string}` is critical for tracking remote pane geometry. The `layout_string` uses tmux's compact layout format (e.g., `177x44,0,0{88x44,0,0,0,88x44,89,0,1}`).

**Boundary tests required:**
- Parse all 18 notification types from raw strings (including `%layout-change`)
- Octal encoding round-trip (`decode(encode(bytes)) == bytes`)
- Response block parsing (`%begin` ... `%end` / `%error`)
- Malformed input resilience (empty lines, unknown types, missing fields, non-numeric timestamps)
- Golden path: replay recorded `tmux -CC` session transcript

### Contract 2: CustomPaneBackend JSON-RPC (Claude Code <-> psmux)

**Request types:**

```rust
pub struct RpcRequest {
    pub id: Option<serde_json::Value>,
    pub method: String,
    pub params: serde_json::Value,
}

// Method-specific params:
pub struct InitializeParams { pub protocol_version: String, pub capabilities: Vec<String> }
pub struct SpawnAgentParams { pub command: Vec<String>, pub cwd: Option<String>, pub env: Option<HashMap<String, String>>, pub metadata: Option<AgentMetadata> }
pub struct WriteParams { pub context_id: String, pub data: String } // base64
pub struct CaptureParams { pub context_id: String, pub lines: Option<u32> }
pub struct KillParams { pub context_id: String }
// list: no params
```

**Response types:**

```rust
pub struct InitializeResult { pub protocol_version: String, pub capabilities: Vec<String>, pub self_context_id: String }
pub struct SpawnAgentResult { pub context_id: String }
pub struct CaptureResult { pub text: String, pub truncated: bool }
pub struct ListResult { pub contexts: Vec<ContextInfo> }
pub struct ContextInfo { pub context_id: String, pub metadata: Option<AgentMetadata> }
```

**Push event:**

```rust
pub struct ContextExitedEvent {
    pub method: String,  // always "context_exited"
    pub params: ContextExitedParams,
}
pub struct ContextExitedParams { pub context_id: String, pub exit_code: Option<i32> }
// No "id" field — this is a notification, not a response
```

**Boundary tests required:**
- All 6 RPC methods deserialize from JSON
- Response round-trip (serialize -> deserialize -> compare)
- Full lifecycle handshake: initialize -> spawn_agent -> list -> write -> capture -> kill
- Edge cases: empty object, missing id, missing method, unknown method, empty argv, nonexistent pane ID, invalid base64, garbage input, negative line count
- `context_exited` event schema validation (no `id` field)
- Contract snapshot testing against recorded Claude Code session

### Contract 3: DCS Passthrough (Child Process -> Host Terminal)

**Input format:** `\x1bPtmux;\x1b<inner_sequence>\x1b\\`
- Inner `\x1b` is doubled (becomes `\x1b\x1b` in the DCS payload)
- ST (String Terminator) is `\x1b\\`

**Boundary tests required:**
- Extract inner sequence from DCS envelope (OSC title change, OSC notification, various inner sequences)
- Non-tmux DCS sequences rejected (not extracted)
- Config gating: `off` suppresses, `on` forwards from active pane only, `all` forwards from any pane
- Empty payload handling

### Contract 4: Canopy <-> CustomPaneBackend (Python side)

**Pydantic models mirror Rust structs:**

```python
class InitializeResult(BaseModel):
    protocol_version: str
    capabilities: list[str]
    self_context_id: str

class SpawnAgentParams(BaseModel):
    command: list[str]
    cwd: str | None = None
    env: dict[str, str] | None = None
    metadata: dict[str, str] | None = None

class SpawnAgentResult(BaseModel):
    context_id: str

class CaptureResult(BaseModel):
    text: str

class ContextExitedEvent(BaseModel):
    method: str
    params: dict
```

**Boundary tests required:**
- Schema validation: psmux responses deserialize into Canopy's Pydantic models
- Round-trip: Canopy's SpawnAgentParams serializes to valid psmux input
- Handshake sequence with recorded fixtures
- Edge cases: empty command, missing context_id, nonexistent pane, invalid base64

### Test Fixtures

| Fixture | Format | Source | Purpose |
|---------|--------|--------|---------|
| `tests/fixtures/tmux_cc_session.txt` | Raw text | Recorded from `tmux -CC` on Linux | Golden path for ControlModeParser |
| `tests/fixtures/backend_session.json` | JSON | Recorded from mock Claude Code client | Golden path for RpcDispatcher |
| `tests/fixtures/dcs_passthrough_samples.bin` | Binary | Hand-crafted DCS sequences | DCS parser edge cases |
| `tests/fixtures/control_mode_edge_cases.txt` | Raw text | Hand-crafted malformed lines | Parser resilience |

### CI Integration

Boundary contract tests run on every PR before integration tests:

```yaml
boundary-contracts:
  runs-on: windows-latest
  steps:
    - uses: actions/checkout@v4
    - run: cargo test --lib -- boundary_tests
    - run: cargo test --test control_mode_contracts
    - run: cargo test --test backend_contracts
```

## 10. Testing Strategy

### tmux Control Mode Client

| Type | How | What |
|------|-----|------|
| Unit: ControlModeParser | `#[cfg(test)]`, feed raw strings | All 17 notifications, octal decoding, blocks, malformed input |
| Unit: RemotePaneManager | Mock parser output | `%output` routing, `%window-add`/close, pane ID mapping |
| Integration: SSH round-trip | Linux VM or WSL with tmux | `psmux attach-remote localhost` through WSL |
| Integration: Reconnect | Kill SSH mid-session | Detect drop, show "reconnecting...", re-attach |
| Mock SSH for CI | Recorded control mode transcript | Replay against parser |

### CustomPaneBackend

| Type | How | What |
|------|-----|------|
| Unit: RpcDispatcher | Feed JSON-RPC strings | All 7 methods, errors, malformed JSON, unknown methods |
| Unit: Method mapping | Mock CtrlReq channel | `spawn_agent` -> SplitWindow, `write` -> SendText, etc. |
| Integration: Named pipe | Full pipe round-trip | `initialize` -> `spawn_agent` -> `write` -> `capture` -> `kill` |
| Integration: context_exited | Spawn agent, let it exit | Verify push event timing and content |
| Compatibility: Claude Code mock | Script mimicking Claude Code | Exact handshake + spawn + monitor + cleanup flow |

### DCS Passthrough

| Type | How | What |
|------|-----|------|
| Unit: vt100-psmux DCS | Feed DCS bytes to parser | Callback fires with correct inner sequence |
| Unit: passthrough queue | Mock pane, inject DCS | Active vs inactive pane, on/off/all config |
| Integration: end-to-end | `printf` DCS inside psmux pane | Host terminal title changes when passthrough on |

## 11. Implementation Phases

### Phase 1: Foundation (parallel tracks)

**Track A — Control Mode Parser** (no SSH, pure protocol):
1. `src/remote/protocol.rs` — `ControlModeMessage` enum + parser
2. `src/remote/octal.rs` — octal encode/decode
3. Boundary contract tests + golden transcript fixture
4. `src/remote/pane_manager.rs` — `RemotePaneManager` with `HashMap<String, vt100::Parser>`

**Track B — CustomPaneBackend**:
1. `src/backend/protocol.rs` — JSON-RPC request/response types
2. `src/backend/pipe.rs` — Named pipe listener + per-client threads
3. `src/backend/dispatcher.rs` — Method routing to CtrlReq
4. Wire into `run_server()` alongside TCP listener
5. Boundary contract tests + lifecycle test

**Track C — DCS Passthrough**:
1. Patch `crates/vt100-psmux/src/perform.rs` — DCS handlers
2. Add `dcs_passthrough` callback
3. Add `PassthroughQueue` to pane struct
4. Drain queue in client render loop
5. Boundary tests

### Phase 2: Integration

**Track A continued:**
5. `src/remote/ssh.rs` — SshTransport (spawn, stdin/stdout, reconnect)
6. `src/remote/mod.rs` — `run_remote_tmux()` function
7. Wire into `src/client.rs` rendering loop
8. CLI commands: `attach-remote`, `new-session-remote`, `list-sessions-remote`
9. Integration tests with WSL/Linux

**Track B continued:**
6. `context_exited` push events (wire to pane death detection)
7. Pipe path discovery file (`~/.psmux/{session}.pipe`)
8. Integration test with mock Claude Code client

### Phase 3: Polish + Canopy

- SSH reconnection with backoff (stale pane content stays visible with status bar "Reconnecting... attempt N/10")
- Flow control (`%pause`/`%continue`) handling
- Error reporting for SSH failures
- Canopy `psmux_rpc.py` client
- Canopy StopFailure hook
- Agent frontmatter updates (`effort`, `maxTurns`, `disallowedTools`)
- Documentation updates (README, CLAUDE.md)

### Phase 4: Remote Execution (Canopy)

- Canopy remote config + router changes
- Remote worktree creation via SSH
- Remote sentinel file monitoring (batched SSH)
- Remote pane spawning via psmux
- Remote worktree cleanup in GC

## 12. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| tmux control mode protocol undocumented edge cases | Medium | Medium | Golden transcript testing; handle unknown notifications gracefully |
| CustomPaneBackend proposal changes before Anthropic ships | Medium | Low | Protocol is simple enough to adapt; our 7-op mapping is 1:1 |
| `vt100-psmux` fork doesn't expose DCS hooks | Medium | Medium | Check vte crate's `Perform` trait; may need small patch to fork |
| Named pipe auth insufficient for multi-user systems | Low | Low | Windows ACLs are sufficient for single-user dev machines |
| SSH key/auth issues on Windows | Medium | Low | Delegate to system SSH; `--ssh-opts` flag for customization |
| ConPTY + control mode interaction quirks | Low | Medium | Control mode bypasses ConPTY entirely (SSH child, not PTY) |
| tmux version differences on remote host | Medium | Low | Parser ignores unknown notification types gracefully; minimum supported version: tmux 3.2+ (for `%layout-change` and flow control). Older tmux detected at connect time via `tmux -V` and warned. |

## 13. Out of Scope

- Hybrid local+remote panes in same window (Phase 2 of a future design)
- Multi-host remote sessions (connect to multiple Linux VMs simultaneously)
- Remote psmux-to-psmux protocol (porting psmux to Linux)
- TLS encryption for psmux TCP protocol (SSH tunnels cover this)
- Plugin system (Lua or WASM)
- Floating panes
