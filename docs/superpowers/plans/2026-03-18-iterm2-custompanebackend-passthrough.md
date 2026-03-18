# iTerm2-for-Windows + CustomPaneBackend + DCS Passthrough — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add remote tmux rendering, Claude Code agent backend, and DCS passthrough to psmux on ohboy-builds.

**Architecture:** Three independent tracks that share zero code paths. Track C (DCS passthrough, ~80 LOC) patches the VT parser. Track B (CustomPaneBackend, ~300 LOC) adds a JSON-RPC named pipe server alongside the existing TCP listener. Track A (control mode client, ~800-1000 LOC) adds a new client mode that connects to remote tmux via SSH. All tracks reuse the existing server event loop, renderer, and pane infrastructure.

**Tech Stack:** Rust stable, `vte` 0.15.0 (DCS hooks), `windows-sys` (named pipes), `serde_json` (JSON-RPC), `base64` (write encoding), existing `vt100-psmux` fork, existing `portable-pty-psmux` fork.

**Spec:** `docs/superpowers/specs/2026-03-18-iterm2-for-windows-and-custompanebackend-design.md`

**Prerequisites:** Before starting any track, create `src/lib.rs` to make psmux a mixed lib+bin crate so integration tests can import modules:

```rust
// src/lib.rs
pub mod backend;
pub mod remote;
pub mod types;
// Re-export only what integration tests need
```

Also add `#[allow(unused)]` annotations on intermediate builds — remove them in the final wiring task of each track. Clippy's `-D warnings` will reject unused functions during incremental development.

---

## File Map

### Track C — DCS Passthrough (smallest, do first)

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `crates/vt100-psmux/src/callbacks.rs` | Add `dcs_passthrough` callback method |
| Modify | `crates/vt100-psmux/src/perform.rs:33-249` | Add `hook()`/`put()`/`unhook()` DCS handlers to `impl Perform` |
| Modify | `src/types.rs:462` | Add `PassthroughQueue` struct, add field to `Pane` |
| Modify | `src/pane.rs:1244-1298` | Wire DCS callback to passthrough queue in reader thread |
| Modify | `src/client.rs` | Drain passthrough queue after frame render, write raw to stdout |
| Create | `tests/passthrough_tests.rs` | Integration tests for DCS forwarding |

### Track B — CustomPaneBackend JSON-RPC

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `src/backend/mod.rs` | Module root, re-exports |
| Create | `src/backend/protocol.rs` | JSON-RPC request/response/event serde types |
| Create | `src/backend/pipe.rs` | Named pipe listener, per-client connection threads |
| Create | `src/backend/dispatcher.rs` | Route JSON-RPC methods to `CtrlReq` variants |
| Modify | `src/server/mod.rs:559-568` | Start pipe listener alongside TCP accept thread |
| Modify | `src/types.rs` | Add pipe path to session discovery files |
| Modify | `src/main.rs` | Wire `CLAUDE_PANE_BACKEND_SOCKET` env var |
| Modify | `Cargo.toml:25-31` | Add `Win32_System_Pipes` feature to `windows-sys` |
| Create | `tests/backend_contracts.rs` | Boundary contract tests |
| Create | `tests/backend_lifecycle.rs` | Full lifecycle integration test |

### Track A — tmux Control Mode Client

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `src/remote/mod.rs` | Module root, `run_remote_tmux()` entry point |
| Create | `src/remote/protocol.rs` | `ControlModeMessage` enum, `parse_control_mode_line()` |
| Create | `src/remote/octal.rs` | Octal encode/decode for tmux wire format |
| Create | `src/remote/parser.rs` | `ControlModeParser` state machine (Idle/InBlock) |
| Create | `src/remote/pane_manager.rs` | `RemotePaneManager` — pane ID → vt100::Parser map |
| Create | `src/remote/ssh.rs` | `SshTransport` — spawn SSH, stdin/stdout, reconnect |
| Modify | `src/client.rs` | Add `run_remote_tmux()` rendering loop |
| Modify | `src/main.rs:248` | Add `attach-remote`, `new-session-remote`, `list-sessions-remote` subcommands |
| Create | `tests/fixtures/tmux_cc_session.txt` | Recorded tmux -CC transcript for golden tests |
| Create | `tests/control_mode_contracts.rs` | Boundary contract tests for parser |
| Create | `tests/remote_integration.rs` | SSH round-trip tests (requires WSL/Linux) |

---

## Track C: DCS Passthrough

### Task 1: Add DCS Passthrough Callback to vt100-psmux

**Files:**
- Modify: `crates/vt100-psmux/src/callbacks.rs:1-70`
- Test: inline `#[cfg(test)]` in same file

- [ ] **Step 1: Write the failing test**

Add to `crates/vt100-psmux/src/callbacks.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct TestCallbacks {
        passthrough_data: Vec<Vec<u8>>,
    }

    impl Callbacks for TestCallbacks {
        fn dcs_passthrough(&mut self, _screen: &mut crate::Screen, data: &[u8]) {
            self.passthrough_data.push(data.to_vec());
        }
    }

    #[test]
    fn test_dcs_passthrough_callback_exists() {
        let mut cb = TestCallbacks { passthrough_data: vec![] };
        // Simulate calling the callback — this just tests the trait compiles
        let mut screen = crate::Screen::new(80, 24);
        cb.dcs_passthrough(&mut screen, b"\x1b]0;title\x07");
        assert_eq!(cb.passthrough_data.len(), 1);
        assert_eq!(cb.passthrough_data[0], b"\x1b]0;title\x07");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vt100-psmux -- tests::test_dcs_passthrough_callback_exists -v`
Expected: FAIL — `dcs_passthrough` method doesn't exist on `Callbacks` trait

- [ ] **Step 3: Add `dcs_passthrough` method to Callbacks trait**

In `crates/vt100-psmux/src/callbacks.rs`, add after the `unhandled_osc` method:

```rust
    fn dcs_passthrough(&mut self, _: &mut crate::Screen, _data: &[u8]) {}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p vt100-psmux -- tests::test_dcs_passthrough_callback_exists -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/vt100-psmux/src/callbacks.rs
git commit -m "feat(vt100): add dcs_passthrough callback to Callbacks trait"
```

### Task 2: Implement DCS Handlers in Perform Trait

**Files:**
- Modify: `crates/vt100-psmux/src/perform.rs:33-249`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write the failing test**

Add to bottom of `crates/vt100-psmux/src/perform.rs`:

```rust
#[cfg(test)]
mod dcs_tests {
    use std::sync::{Arc, Mutex};

    struct DcsCapture {
        captured: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl crate::callbacks::Callbacks for DcsCapture {
        fn dcs_passthrough(&mut self, _: &mut crate::Screen, data: &[u8]) {
            self.captured.lock().unwrap().push(data.to_vec());
        }
    }

    #[test]
    fn test_dcs_tmux_passthrough_detected() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let callbacks = DcsCapture { captured: captured.clone() };
        let mut parser = crate::Parser::new_with_callbacks(80, 24, 0, callbacks);

        // DCS tmux passthrough: ESC P tmux; ESC ESC ] 0 ; title BEL ESC backslash
        let input = b"\x1bPtmux;\x1b\x1b]0;My Title\x07\x1b\\";
        parser.process(input);

        let data = captured.lock().unwrap();
        assert!(!data.is_empty(), "DCS tmux passthrough should trigger callback");
    }

    #[test]
    fn test_non_tmux_dcs_ignored() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let callbacks = DcsCapture { captured: captured.clone() };
        let mut parser = crate::Parser::new_with_callbacks(80, 24, 0, callbacks);

        // Non-tmux DCS (e.g., sixel)
        let input = b"\x1bP0;1;0q\x1b\\";
        parser.process(input);

        let data = captured.lock().unwrap();
        assert!(data.is_empty(), "Non-tmux DCS should not trigger passthrough callback");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p vt100-psmux -- dcs_tests -v`
Expected: FAIL — `new_with_callbacks` may not exist yet, or DCS hooks not implemented

- [ ] **Step 3: Implement DCS hook/put/unhook on WrappedScreen**

In `crates/vt100-psmux/src/perform.rs`, add inside the `impl Perform for WrappedScreen` block (after `osc_dispatch`):

```rust
    fn hook(&mut self, params: &vte::Params, intermediates: &[u8], _ignore: bool, _action: char) {
        // Start of DCS sequence — buffer the params to check for "tmux;"
        self.dcs_buf.clear();
        self.dcs_is_tmux = false;
        // DCS tmux passthrough starts with: ESC P tmux; ...
        // vte parses DCS params and gives us intermediates
        // The "tmux;" prefix comes as the initial put() bytes
    }

    fn put(&mut self, byte: u8) {
        // Accumulate DCS payload
        self.dcs_buf.push(byte);
        // Check for "tmux;" prefix after 5 bytes
        if self.dcs_buf.len() == 5 && &self.dcs_buf[..5] == b"tmux;" {
            self.dcs_is_tmux = true;
            self.dcs_buf.clear(); // Clear prefix, keep only inner payload
        }
    }

    fn unhook(&mut self) {
        // End of DCS sequence
        if self.dcs_is_tmux && !self.dcs_buf.is_empty() {
            // Unescape doubled ESC in tmux passthrough (ESC ESC -> ESC)
            let inner = unescape_tmux_passthrough(&self.dcs_buf);
            self.callbacks.dcs_passthrough(&mut self.screen, &inner);
        }
        self.dcs_buf.clear();
        self.dcs_is_tmux = false;
    }
```

Add fields to `WrappedScreen` struct:
```rust
    dcs_buf: Vec<u8>,
    dcs_is_tmux: bool,
```

Add helper function:
```rust
fn unescape_tmux_passthrough(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if i + 1 < data.len() && data[i] == 0x1b && data[i + 1] == 0x1b {
            result.push(0x1b); // doubled ESC -> single ESC
            i += 2;
        } else {
            result.push(data[i]);
            i += 1;
        }
    }
    result
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p vt100-psmux -- dcs_tests -v`
Expected: PASS (both tests)

- [ ] **Step 5: Commit**

```bash
git add crates/vt100-psmux/src/perform.rs
git commit -m "feat(vt100): implement DCS hook/put/unhook for tmux passthrough"
```

### Task 3: Add PassthroughQueue and Wire to Pane

**Files:**
- Modify: `src/types.rs:462`
- Modify: `src/pane.rs:1244-1298`
- Test: `tests/passthrough_tests.rs`

- [ ] **Step 1: Write the failing test**

Create `tests/passthrough_tests.rs`:

```rust
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct PassthroughQueue {
    entries: Arc<Mutex<Vec<Vec<u8>>>>,
    max_depth: usize,
}

impl PassthroughQueue {
    fn new(max_depth: usize) -> Self {
        Self { entries: Arc::new(Mutex::new(Vec::new())), max_depth }
    }

    fn push(&self, data: Vec<u8>) {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= self.max_depth {
            entries.remove(0); // oldest-discard
        }
        entries.push(data);
    }

    fn drain(&self) -> Vec<Vec<u8>> {
        let mut entries = self.entries.lock().unwrap();
        std::mem::take(&mut *entries)
    }
}

#[test]
fn test_passthrough_queue_basic() {
    let q = PassthroughQueue::new(64);
    q.push(b"\x1b]0;title\x07".to_vec());
    let drained = q.drain();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0], b"\x1b]0;title\x07");
    // After drain, queue is empty
    assert!(q.drain().is_empty());
}

#[test]
fn test_passthrough_queue_max_depth() {
    let q = PassthroughQueue::new(3);
    q.push(b"a".to_vec());
    q.push(b"b".to_vec());
    q.push(b"c".to_vec());
    q.push(b"d".to_vec()); // should evict "a"
    let drained = q.drain();
    assert_eq!(drained.len(), 3);
    assert_eq!(drained[0], b"b");
    assert_eq!(drained[2], b"d");
}

#[test]
fn test_passthrough_gated_on_config() {
    // allow-passthrough "off" -> never forward
    // allow-passthrough "on" -> forward only from active pane
    // allow-passthrough "all" -> forward from any pane
    let q = PassthroughQueue::new(64);
    let data = b"\x1b]0;test\x07".to_vec();

    // "off" config — should_forward returns false regardless
    assert!(!should_forward("off", true));
    assert!(!should_forward("off", false));

    // "on" config — only active pane
    assert!(should_forward("on", true));
    assert!(!should_forward("on", false));

    // "all" config — any pane
    assert!(should_forward("all", true));
    assert!(should_forward("all", false));
}

fn should_forward(config: &str, is_active_pane: bool) -> bool {
    match config {
        "all" => true,
        "on" => is_active_pane,
        _ => false,
    }
}
```

- [ ] **Step 2: Run tests to verify they pass** (these are standalone — just validate the data structures)

Run: `cargo test --test passthrough_tests -v`
Expected: PASS (these test the queue logic in isolation)

- [ ] **Step 3: Add PassthroughQueue to types.rs and Pane struct**

In `src/types.rs`, add the `PassthroughQueue` struct (near line 462 by `allow_passthrough`):

```rust
#[derive(Clone)]
pub struct PassthroughQueue {
    entries: Arc<Mutex<Vec<Vec<u8>>>>,
    max_depth: usize,
}

impl PassthroughQueue {
    pub fn new(max_depth: usize) -> Self {
        Self { entries: Arc::new(Mutex::new(Vec::new())), max_depth }
    }

    pub fn push(&self, data: Vec<u8>) {
        if let Ok(mut entries) = self.entries.lock() {
            if entries.len() >= self.max_depth {
                entries.remove(0);
            }
            entries.push(data);
        }
    }

    pub fn drain(&self) -> Vec<Vec<u8>> {
        self.entries.lock().ok()
            .map(|mut e| std::mem::take(&mut *e))
            .unwrap_or_default()
    }
}
```

Add to `Pane` struct (in types.rs):
```rust
    pub passthrough_queue: PassthroughQueue,
```

Initialize in pane creation with `PassthroughQueue::new(64)`.

- [ ] **Step 4: Wire DCS callback to passthrough queue in spawn_reader_thread**

In `src/pane.rs`, the reader thread at line 1266 calls `parser.process(&local[..n])`. The DCS callback fires inside `process()`. Need to pass the `PassthroughQueue` as the callback receiver.

This requires creating a callbacks struct that holds a reference to the queue:

```rust
struct PaneCallbacks {
    passthrough_queue: PassthroughQueue,
    allow_passthrough: Arc<Mutex<String>>,
    is_active: Arc<AtomicBool>,
}

impl vt100_psmux::Callbacks for PaneCallbacks {
    fn dcs_passthrough(&mut self, _screen: &mut vt100_psmux::Screen, data: &[u8]) {
        let config = self.allow_passthrough.lock().map(|s| s.clone()).unwrap_or_default();
        let active = self.is_active.load(Ordering::Relaxed);
        if should_forward(&config, active) {
            self.passthrough_queue.push(data.to_vec());
        }
    }
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test --test passthrough_tests -v && cargo test -p vt100-psmux -v`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/types.rs src/pane.rs tests/passthrough_tests.rs
git commit -m "feat: add PassthroughQueue and wire DCS callback to pane reader"
```

### Task 4: Forward Passthrough in Client Render Loop

**Files:**
- Modify: `src/client.rs` (render loop area)

- [ ] **Step 1: Identify insertion point**

In `src/client.rs:run_remote()`, after the frame is rendered to the terminal (after `terminal.draw()`), add passthrough drain logic.

- [ ] **Step 2: Add passthrough forwarding code**

After the frame render in the client's main loop:

```rust
// Drain passthrough queue for active pane and write raw to stdout
if let Some(active_pane_passthrough) = get_active_pane_passthrough(&layout) {
    let sequences = active_pane_passthrough.drain();
    if !sequences.is_empty() {
        let mut stdout = std::io::stdout().lock();
        for seq in sequences {
            let _ = stdout.write_all(&seq);
        }
        let _ = stdout.flush();
    }
}
```

Note: The passthrough data must bypass ratatui — it goes directly to the host terminal's stdout as raw bytes. This is the same pattern used for OSC 52 clipboard and DECSCUSR cursor shape.

- [ ] **Step 3: Build and verify no regressions**

Run: `cargo build && cargo test`
Expected: Build succeeds, all existing tests pass

- [ ] **Step 4: Manual test**

In a psmux session with `set -g allow-passthrough on`:
```bash
printf '\ePtmux;\e\e]0;TEST TITLE\a\e\\'
```
Expected: Host terminal title changes to "TEST TITLE"

- [ ] **Step 5: Commit**

```bash
git add src/client.rs
git commit -m "feat: forward DCS tmux passthrough to host terminal"
```

---

## Track B: CustomPaneBackend JSON-RPC Server

### Task 5: Define JSON-RPC Protocol Types

**Files:**
- Create: `src/backend/mod.rs`
- Create: `src/backend/protocol.rs`
- Test: `tests/backend_contracts.rs`

- [ ] **Step 1: Create module structure**

```bash
mkdir -p src/backend
```

Create `src/backend/mod.rs`:
```rust
pub mod protocol;
pub mod pipe;
pub mod dispatcher;
```

Add `mod backend;` to `src/lib.rs` or `src/main.rs` (wherever modules are declared).

- [ ] **Step 2: Write boundary contract tests**

Create `tests/backend_contracts.rs`:

```rust
use serde::{Deserialize, Serialize};
use serde_json::json;

// Mirror types from src/backend/protocol.rs (will be imported once created)

#[derive(Debug, Deserialize)]
struct RpcRequest {
    id: Option<serde_json::Value>,
    method: String,
    params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct InitializeResult {
    protocol_version: String,
    capabilities: Vec<String>,
    self_context_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SpawnAgentResult {
    context_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CaptureResult {
    text: String,
    truncated: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ListResult {
    contexts: Vec<ContextInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ContextInfo {
    context_id: String,
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ContextExitedEvent {
    method: String,
    params: ContextExitedParams,
}

#[derive(Debug, Serialize, Deserialize)]
struct ContextExitedParams {
    context_id: String,
    exit_code: Option<i32>,
}

#[test]
fn test_all_rpc_methods_deserialize() {
    let cases = vec![
        (r#"{"id":"1","method":"initialize","params":{"protocol_version":"1","capabilities":["events"]}}"#, "initialize"),
        (r#"{"id":"2","method":"spawn_agent","params":{"command":["claude","--agent"],"cwd":"/project"}}"#, "spawn_agent"),
        (r#"{"id":"3","method":"write","params":{"context_id":"%1","data":"aGVsbG8="}}"#, "write"),
        (r#"{"id":"4","method":"capture","params":{"context_id":"%1","lines":200}}"#, "capture"),
        (r#"{"id":"5","method":"kill","params":{"context_id":"%1"}}"#, "kill"),
        (r#"{"id":"6","method":"list","params":{}}"#, "list"),
    ];
    for (json_str, expected_method) in cases {
        let req: RpcRequest = serde_json::from_str(json_str)
            .unwrap_or_else(|e| panic!("Failed to parse {expected_method}: {e}"));
        assert_eq!(req.method, expected_method);
    }
}

#[test]
fn test_initialize_response_roundtrip() {
    let result = InitializeResult {
        protocol_version: "1".into(),
        capabilities: vec!["events".into(), "capture".into()],
        self_context_id: "%0".into(),
    };
    let json = serde_json::to_string(&result).unwrap();
    let parsed: InitializeResult = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.self_context_id, "%0");
    assert_eq!(parsed.capabilities.len(), 2);
}

#[test]
fn test_context_exited_event_has_no_id() {
    let event = ContextExitedEvent {
        method: "context_exited".into(),
        params: ContextExitedParams {
            context_id: "%3".into(),
            exit_code: Some(0),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.get("id").is_none(), "Push events must not have id field");
    assert_eq!(parsed["method"], "context_exited");
}

#[test]
fn test_rpc_edge_cases() {
    let edge_cases = vec![
        r#"{}"#,
        r#"{"method":"initialize"}"#,
        r#"{"id":"1"}"#,
        r#"not json"#,
        r#"{"id":"1","method":"unknown","params":{}}"#,
    ];
    for input in edge_cases {
        let result = serde_json::from_str::<RpcRequest>(input);
        // Should either parse or fail gracefully — never panic
        let _ = result;
    }
}

#[test]
fn test_capture_result_truncated_flag() {
    let full = CaptureResult { text: "hello".into(), truncated: false };
    let truncated = CaptureResult { text: "hel...".into(), truncated: true };
    let full_json = serde_json::to_string(&full).unwrap();
    let trunc_json = serde_json::to_string(&truncated).unwrap();
    assert!(full_json.contains("\"truncated\":false"));
    assert!(trunc_json.contains("\"truncated\":true"));
}
```

- [ ] **Step 3: Run tests to verify they pass** (serde tests work with local types)

Run: `cargo test --test backend_contracts -v`
Expected: PASS

- [ ] **Step 4: Create `src/backend/protocol.rs` with canonical types**

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// --- Requests ---

#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct InitializeParams {
    pub protocol_version: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SpawnAgentParams {
    pub command: Vec<String>,
    pub cwd: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub metadata: Option<AgentMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetadata {
    pub name: Option<String>,
    pub color: Option<String>,
    pub role: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WriteParams {
    pub context_id: String,
    pub data: String, // base64 encoded
}

#[derive(Debug, Deserialize)]
pub struct CaptureParams {
    pub context_id: String,
    pub lines: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct KillParams {
    pub context_id: String,
}

// --- Responses ---

#[derive(Debug, Serialize)]
pub struct RpcResponse {
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: Vec<String>,
    pub self_context_id: String,
}

#[derive(Debug, Serialize)]
pub struct SpawnAgentResult {
    pub context_id: String,
}

#[derive(Debug, Serialize)]
pub struct CaptureResult {
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct ListResult {
    pub contexts: Vec<ContextInfo>,
}

#[derive(Debug, Serialize)]
pub struct ContextInfo {
    pub context_id: String,
    pub metadata: Option<AgentMetadata>,
}

// --- Push Events ---

#[derive(Debug, Serialize)]
pub struct ContextExitedEvent {
    pub method: String, // always "context_exited"
    pub params: ContextExitedParams,
}

#[derive(Debug, Serialize)]
pub struct ContextExitedParams {
    pub context_id: String,
    pub exit_code: Option<i32>,
}

impl RpcResponse {
    pub fn success(id: serde_json::Value, result: impl Serialize) -> Self {
        Self {
            id,
            result: Some(serde_json::to_value(result).unwrap_or_default()),
            error: None,
        }
    }

    pub fn error(id: serde_json::Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            id,
            result: None,
            error: Some(RpcError { code, message: message.into() }),
        }
    }
}
```

- [ ] **Step 5: Update tests to import from protocol module, run again**

Run: `cargo test --test backend_contracts -v && cargo build`
Expected: PASS + clean build

- [ ] **Step 6: Commit**

```bash
git add src/backend/mod.rs src/backend/protocol.rs tests/backend_contracts.rs
git commit -m "feat(backend): define CustomPaneBackend JSON-RPC protocol types"
```

### Task 6: Named Pipe Listener

**Files:**
- Create: `src/backend/pipe.rs`
- Modify: `Cargo.toml:25-31`

- [ ] **Step 1: Add Win32 pipe feature to Cargo.toml**

In `Cargo.toml`, add `base64` dependency and update `windows-sys` features:

```toml
[dependencies]
# ... existing deps ...
base64 = "0.22"

[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.61", features = [
    "Win32_Foundation",
    "Win32_System_Memory",
    "Win32_System_DataExchange",
    "Win32_System_Pipes",
    "Win32_Storage_FileSystem",
    "Win32_Security",
] }
```

- [ ] **Step 2: Implement named pipe listener**

Create `src/backend/pipe.rs`:

```rust
use std::io::{self, BufRead, BufReader, Write};
use std::sync::mpsc;
use std::thread;

use crate::types::CtrlReq;

/// Named pipe path for a session
pub fn pipe_path(session_name: &str) -> String {
    format!(r"\\.\pipe\psmux-claude-backend-{}", session_name)
}

/// Start listening on a named pipe for JSON-RPC connections.
/// Spawns a thread per connection, dispatches to the server via `tx`.
pub fn start_pipe_listener(
    session_name: &str,
    tx: mpsc::Sender<CtrlReq>,
    session_key: String,
) -> io::Result<()> {
    let pipe_name = pipe_path(session_name);
    let pipe_name_clone = pipe_name.clone();

    thread::spawn(move || {
        loop {
            // Create named pipe instance and wait for client
            match create_and_wait_for_client(&pipe_name_clone) {
                Ok((reader, writer)) => {
                    let tx = tx.clone();
                    let key = session_key.clone();
                    thread::spawn(move || {
                        if let Err(e) = handle_rpc_connection(reader, writer, tx) {
                            eprintln!("RPC connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Pipe accept error: {}", e);
                    thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
    });

    // Write pipe path to discovery file
    let dir = crate::session::psmux_dir();
    let pipe_file = format!("{}\\{}.pipe", dir, session_name);
    let _ = std::fs::write(&pipe_file, &pipe_name);

    Ok(())
}

#[cfg(windows)]
fn create_and_wait_for_client(
    pipe_name: &str,
) -> io::Result<(impl BufRead, impl Write)> {
    use windows_sys::Win32::System::Pipes::*;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Storage::FileSystem::*;

    let wide_name: Vec<u16> = pipe_name.encode_utf16().chain(std::iter::once(0)).collect();

    let handle = unsafe {
        CreateNamedPipeW(
            wide_name.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            PIPE_UNLIMITED_INSTANCES,
            65536, // out buffer
            65536, // in buffer
            0,     // default timeout
            std::ptr::null(),
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }

    // Wait for client connection
    let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
    if connected == 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
            return Err(err);
        }
    }

    // Wrap handle in std::fs::File for Read/Write
    use std::os::windows::io::FromRawHandle;
    let file = unsafe { std::fs::File::from_raw_handle(handle as *mut _) };
    let reader = BufReader::new(file.try_clone()?);
    let writer = file;

    Ok((reader, writer))
}

fn handle_rpc_connection(
    reader: impl BufRead,
    writer: impl Write + Send + 'static,
    tx: mpsc::Sender<CtrlReq>,
) -> io::Result<()> {
    use super::dispatcher::dispatch_rpc;

    let writer = std::sync::Mutex::new(writer);

    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let response = dispatch_rpc(&line, &tx);
        if let Some(resp_json) = response {
            let mut w = writer.lock().unwrap();
            writeln!(w, "{}", resp_json)?;
            w.flush()?;
        }
    }

    Ok(())
}
```

- [ ] **Step 3: Build to verify compilation**

Run: `cargo build`
Expected: Build succeeds (pipe module compiles)

- [ ] **Step 4: Commit**

```bash
git add src/backend/pipe.rs Cargo.toml
git commit -m "feat(backend): named pipe listener for JSON-RPC connections"
```

### Task 7: RPC Dispatcher — Method Routing

**Files:**
- Create: `src/backend/dispatcher.rs`
- Test: `tests/backend_lifecycle.rs`

- [ ] **Step 1: Write the lifecycle test**

Create `tests/backend_lifecycle.rs`:

```rust
// This test validates the dispatcher's method routing logic
// using a mock CtrlReq channel (no real psmux server needed)

use std::sync::mpsc;

#[test]
fn test_dispatch_initialize() {
    let (tx, _rx) = mpsc::channel();
    let input = r#"{"id":"1","method":"initialize","params":{"protocol_version":"1","capabilities":["events"]}}"#;

    let response = psmux::backend::dispatcher::dispatch_rpc(input, &tx);
    assert!(response.is_some());

    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert_eq!(parsed["id"], "1");
    assert!(parsed["result"]["self_context_id"].is_string());
    assert_eq!(parsed["result"]["protocol_version"], "1");
}

#[test]
fn test_dispatch_unknown_method() {
    let (tx, _rx) = mpsc::channel();
    let input = r#"{"id":"1","method":"nonexistent","params":{}}"#;

    let response = psmux::backend::dispatcher::dispatch_rpc(input, &tx);
    assert!(response.is_some());

    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert!(parsed["error"].is_object());
    assert_eq!(parsed["error"]["code"], -32601); // Method not found
}

#[test]
fn test_dispatch_malformed_json() {
    let (tx, _rx) = mpsc::channel();
    let response = psmux::backend::dispatcher::dispatch_rpc("not json", &tx);
    assert!(response.is_some());

    let parsed: serde_json::Value = serde_json::from_str(&response.unwrap()).unwrap();
    assert!(parsed["error"].is_object());
    assert_eq!(parsed["error"]["code"], -32700); // Parse error
}
```

- [ ] **Step 2: Create dispatcher**

Create `src/backend/dispatcher.rs`:

```rust
use std::sync::mpsc;
use crate::types::CtrlReq;
use super::protocol::*;

/// Dispatch a single JSON-RPC line. Returns JSON response string (or None for notifications).
pub fn dispatch_rpc(
    line: &str,
    tx: &mpsc::Sender<CtrlReq>,
) -> Option<String> {
    let req: RpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            let resp = RpcResponse::error(
                serde_json::Value::Null,
                -32700,
                format!("Parse error: {e}"),
            );
            return Some(serde_json::to_string(&resp).unwrap());
        }
    };

    let id = req.id.clone().unwrap_or(serde_json::Value::Null);

    let result = match req.method.as_str() {
        "initialize" => handle_initialize(&req.params, tx),
        "spawn_agent" => handle_spawn_agent(&req.params, tx),
        "write" => handle_write(&req.params, tx),
        "capture" => handle_capture(&req.params, tx),
        "kill" => handle_kill(&req.params, tx),
        "list" => handle_list(&req.params, tx),
        _ => Err((-32601, format!("Method not found: {}", req.method))),
    };

    let resp = match result {
        Ok(value) => RpcResponse::success(id, value),
        Err((code, msg)) => RpcResponse::error(id, code, msg),
    };

    Some(serde_json::to_string(&resp).unwrap())
}

fn handle_initialize(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, (i32, String)> {
    let _params: InitializeParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    // Get active pane ID from server state via CtrlReq channel
    let (resp_tx, resp_rx) = mpsc::channel::<String>();
    let _ = tx.send(CtrlReq::BackendInitialize { resp: resp_tx });
    let self_context_id = resp_rx.recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| (-32603, "Initialize timeout".into()))?;

    let result = InitializeResult {
        protocol_version: "1".into(),
        capabilities: vec!["events".into(), "capture".into()],
        self_context_id,
    };

    serde_json::to_value(result).map_err(|e| (-32603, format!("Internal error: {e}")))
}

fn handle_spawn_agent(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, (i32, String)> {
    let p: SpawnAgentParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    if p.command.is_empty() {
        return Err((-32602, "command must not be empty".into()));
    }

    let (resp_tx, resp_rx) = mpsc::channel::<String>();
    let _ = tx.send(CtrlReq::BackendSpawnAgent {
        command: p.command,
        cwd: p.cwd,
        env: p.env,
        metadata: p.metadata.map(|m| (m.name, m.role)),
        resp: resp_tx,
    });

    let pane_id = resp_rx.recv_timeout(std::time::Duration::from_secs(10))
        .map_err(|_| (-32603, "Spawn timeout".into()))?;

    let result = SpawnAgentResult { context_id: pane_id };
    serde_json::to_value(result).map_err(|e| (-32603, format!("Internal error: {e}")))
}

fn handle_write(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, (i32, String)> {
    let p: WriteParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD.decode(&p.data)
        .map_err(|e| (-32602, format!("Invalid base64: {e}")))?;

    let text = String::from_utf8_lossy(&decoded).into_owned();
    let _ = tx.send(CtrlReq::BackendSendText { pane_id: p.context_id, text });

    Ok(serde_json::json!({}))
}

fn handle_capture(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, (i32, String)> {
    let p: CaptureParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    let (resp_tx, resp_rx) = mpsc::channel::<String>();
    let _ = tx.send(CtrlReq::BackendCapturePane {
        pane_id: p.context_id,
        lines: p.lines,
        clean: true,
        resp: resp_tx,
    });

    let text = resp_rx.recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| (-32603, "Capture timeout".into()))?;

    let max_lines = p.lines.unwrap_or(200) as usize;
    let line_count = text.lines().count();
    let result = CaptureResult {
        text,
        truncated: line_count >= max_lines,
    };
    serde_json::to_value(result).map_err(|e| (-32603, format!("Internal error: {e}")))
}

fn handle_kill(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, (i32, String)> {
    let p: KillParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    let (resp_tx, resp_rx) = mpsc::channel::<()>();
    let _ = tx.send(CtrlReq::BackendKillPane { pane_id: p.context_id, resp: resp_tx });
    let _ = resp_rx.recv_timeout(std::time::Duration::from_secs(5));
    Ok(serde_json::json!({}))
}

fn handle_list(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, (i32, String)> {
    let (resp_tx, resp_rx) = mpsc::channel::<String>();
    let _ = tx.send(CtrlReq::BackendListPanes { resp: resp_tx });

    let json = resp_rx.recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| (-32603, "List timeout".into()))?;

    serde_json::from_str(&json).map_err(|e| (-32603, format!("Internal error: {e}")))
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --test backend_lifecycle -v`
Expected: `test_dispatch_initialize` PASS, others PASS

- [ ] **Step 4: Commit**

```bash
git add src/backend/dispatcher.rs tests/backend_lifecycle.rs
git commit -m "feat(backend): JSON-RPC dispatcher routing methods to CtrlReq"
```

### Task 8: Wire Backend Into Server + Add CtrlReq Variants

**Files:**
- Modify: `src/types.rs` — add new `CtrlReq` variants
- Modify: `src/server/mod.rs:559-568` — start pipe listener
- Modify: `src/server/mod.rs` (main loop) — handle new CtrlReq variants

- [ ] **Step 1: Add new CtrlReq variants to types.rs**

Add to the `CtrlReq` enum:

```rust
    BackendInitialize {
        resp: mpsc::Sender<String>, // returns active pane ID as "%N"
    },
    BackendSpawnAgent {
        command: Vec<String>,
        cwd: Option<String>,
        env: Option<std::collections::HashMap<String, String>>,
        metadata: Option<(Option<String>, Option<String>)>, // (name, role)
        resp: mpsc::Sender<String>,
    },
    BackendCapturePane {
        pane_id: String,
        lines: Option<u32>,
        clean: bool,
        resp: mpsc::Sender<String>,
    },
    BackendListPanes {
        resp: mpsc::Sender<String>,
    },
    BackendKillPane {
        pane_id: String,
        resp: mpsc::Sender<()>,
    },
    BackendSendText {
        pane_id: String,
        text: String,
    },
```

- [ ] **Step 2: Add handlers in server main loop**

In `src/server/mod.rs`, in the main `match req { ... }` block, add handlers for the new variants. Each handler reuses existing logic:

- `BackendSpawnAgent` → reuse the `SplitWindow` handler code, but use `CommandBuilder` with `argv[]` directly
- `BackendCapturePane` → reuse existing `CapturePane` handler
- `BackendListPanes` → reuse existing `ListPanes` JSON handler
- `KillPaneById` → find pane by ID string, kill it
- `SendTextToPane` → find pane by ID, write to PTY

- [ ] **Step 3: Start pipe listener in run_server()**

In `src/server/mod.rs`, after the TCP accept thread spawn (line 568), add:

```rust
// Start CustomPaneBackend named pipe listener
if let Err(e) = crate::backend::pipe::start_pipe_listener(
    &app.session_name,
    tx.clone(),
    session_key.clone(),
) {
    eprintln!("Warning: Failed to start backend pipe: {}", e);
}
```

- [ ] **Step 4: Build and test**

Run: `cargo build && cargo test`
Expected: Clean build, all tests pass

- [ ] **Step 5: Set CLAUDE_PANE_BACKEND_SOCKET env var in pane environment**

In the pane creation code (where `TMUX` and `TMUX_PANE` env vars are set), add:

```rust
env.insert(
    "CLAUDE_PANE_BACKEND_SOCKET".into(),
    crate::backend::pipe::pipe_path(&app.session_name),
);
```

- [ ] **Step 6: Commit**

```bash
git add src/types.rs src/server/mod.rs src/main.rs
git commit -m "feat(backend): wire CustomPaneBackend pipe listener into server"
```

### Task 9: context_exited Push Events

**Files:**
- Modify: `src/backend/pipe.rs` — add event push channel to connections
- Modify: `src/server/mod.rs` — detect pane death, push events

- [ ] **Step 1: Add event push sender to RPC connections**

In `src/backend/pipe.rs`, modify `handle_rpc_connection` to accept an event receiver and spawn a writer thread for push events:

```rust
fn handle_rpc_connection(
    reader: impl BufRead,
    writer: impl Write + Send + 'static,
    tx: mpsc::Sender<CtrlReq>,
) -> io::Result<()> {
    let writer = Arc::new(Mutex::new(writer));
    let writer_clone = writer.clone();

    // Register for push events
    let (event_tx, event_rx) = mpsc::channel::<String>();
    crate::types::register_backend_event_sender(event_tx);

    // Push event writer thread
    thread::spawn(move || {
        while let Ok(event_json) = event_rx.recv() {
            let mut w = writer_clone.lock().unwrap();
            if writeln!(w, "{}", event_json).is_err() { break; }
            if w.flush().is_err() { break; }
        }
    });

    // Request/response loop (existing code)
    for line in reader.lines() { /* ... */ }

    Ok(())
}
```

- [ ] **Step 2: Add event sender registry in types.rs**

```rust
static BACKEND_EVENT_SENDERS: Mutex<Vec<mpsc::Sender<String>>> = Mutex::new(Vec::new());

pub fn register_backend_event_sender(tx: mpsc::Sender<String>) {
    if let Ok(mut v) = BACKEND_EVENT_SENDERS.lock() {
        v.push(tx);
    }
}

pub fn push_backend_event(event_json: &str) {
    if let Ok(mut senders) = BACKEND_EVENT_SENDERS.lock() {
        senders.retain(|tx| tx.send(event_json.to_string()).is_ok());
    }
}
```

- [ ] **Step 3: Push context_exited when pane dies**

In `src/server/mod.rs`, where pane death is detected (in the main loop, where `pane.dead` is set or `child.try_wait()` returns `Some`), add:

```rust
if pane_just_died {
    let event = crate::backend::protocol::ContextExitedEvent {
        method: "context_exited".into(),
        params: crate::backend::protocol::ContextExitedParams {
            context_id: format!("%{}", pane.id),
            exit_code: exit_code.map(|c| c.into()),
        },
    };
    if let Ok(json) = serde_json::to_string(&event) {
        crate::types::push_backend_event(&json);
    }
}
```

- [ ] **Step 4: Test with mock client**

Manual test: connect to pipe with a simple Python/PowerShell script, spawn an agent (`echo hello && exit`), wait for `context_exited` push event.

- [ ] **Step 5: Commit**

```bash
git add src/backend/pipe.rs src/types.rs src/server/mod.rs
git commit -m "feat(backend): push context_exited events when agent panes die"
```

---

## Track A: tmux Control Mode Client

### Task 10: Octal Encoding/Decoding

**Files:**
- Create: `src/remote/octal.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write tests**

Create `src/remote/octal.rs`:

```rust
/// Decode tmux control mode octal encoding.
/// Characters < ASCII 32 and `\` are replaced with `\NNN` (3-digit octal).
pub fn decode_octal(input: &str) -> Vec<u8> {
    let mut result = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len()
            && bytes[i + 1].is_ascii_digit()
            && bytes[i + 2].is_ascii_digit()
            && bytes[i + 3].is_ascii_digit()
        {
            let val = (bytes[i + 1] - b'0') as u8 * 64
                + (bytes[i + 2] - b'0') as u8 * 8
                + (bytes[i + 3] - b'0') as u8;
            result.push(val);
            i += 4;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    result
}

/// Encode bytes to tmux control mode octal encoding.
pub fn encode_octal(input: &[u8]) -> String {
    let mut result = String::with_capacity(input.len() * 2);
    for &b in input {
        if b < 32 || b == b'\\' {
            result.push_str(&format!("\\{:03o}", b));
        } else {
            result.push(b as char);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_simple() {
        assert_eq!(decode_octal("hello"), b"hello");
    }

    #[test]
    fn test_decode_newline() {
        assert_eq!(decode_octal("hello\\015\\012"), b"hello\r\n");
    }

    #[test]
    fn test_decode_backslash() {
        assert_eq!(decode_octal("path\\134file"), b"path\\file");
    }

    #[test]
    fn test_decode_escape() {
        assert_eq!(decode_octal("\\033[32m"), b"\x1b[32m");
    }

    #[test]
    fn test_roundtrip() {
        let original = b"\x1b[32mhello\x1b[0m\r\nworld\\path";
        let encoded = encode_octal(original);
        let decoded = decode_octal(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_decode_empty() {
        assert_eq!(decode_octal(""), b"");
    }

    #[test]
    fn test_decode_partial_octal() {
        // Not enough digits — treat as literal
        assert_eq!(decode_octal("\\01"), b"\\01");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p psmux -- remote::octal::tests -v`
Expected: All PASS

- [ ] **Step 3: Commit**

```bash
git add src/remote/octal.rs
git commit -m "feat(remote): octal encode/decode for tmux control mode"
```

### Task 11: Control Mode Protocol Types

**Files:**
- Create: `src/remote/protocol.rs`
- Test: inline

- [ ] **Step 1: Define ControlModeMessage enum and write tests**

Create `src/remote/protocol.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ControlModeMessage {
    Response {
        timestamp: u64,
        command_id: u64,
        flags: u32,
        body: Vec<String>,
        success: bool,
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

/// Parse a single control mode notification line.
/// Returns None for empty/unrecognized lines (graceful degradation).
pub fn parse_notification(line: &str) -> Option<ControlModeMessage> {
    let line = line.trim();
    if !line.starts_with('%') {
        return None;
    }

    let parts: Vec<&str> = line.splitn(4, ' ').collect();
    let cmd = parts.first()?;

    match *cmd {
        "%output" => {
            let pane_id = parts.get(1)?.to_string();
            let data_str = parts.get(2).unwrap_or(&"");
            let rest = if parts.len() > 3 {
                format!("{} {}", data_str, parts[3])
            } else {
                data_str.to_string()
            };
            Some(ControlModeMessage::Output {
                pane_id,
                data: super::octal::decode_octal(&rest),
            })
        }
        "%window-add" => Some(ControlModeMessage::WindowAdd {
            window_id: parts.get(1)?.to_string(),
        }),
        "%window-close" => Some(ControlModeMessage::WindowClose {
            window_id: parts.get(1)?.to_string(),
        }),
        "%window-renamed" => Some(ControlModeMessage::WindowRenamed {
            window_id: parts.get(1)?.to_string(),
            new_name: parts.get(2).unwrap_or(&"").to_string(),
        }),
        "%unlinked-window-add" => Some(ControlModeMessage::UnlinkedWindowAdd {
            window_id: parts.get(1)?.to_string(),
        }),
        "%unlinked-window-close" => Some(ControlModeMessage::UnlinkedWindowClose {
            window_id: parts.get(1)?.to_string(),
        }),
        "%unlinked-window-renamed" => Some(ControlModeMessage::UnlinkedWindowRenamed {
            window_id: parts.get(1)?.to_string(),
            new_name: parts.get(2).unwrap_or(&"").to_string(),
        }),
        "%session-changed" => Some(ControlModeMessage::SessionChanged {
            session_id: parts.get(1)?.to_string(),
            session_name: parts.get(2).unwrap_or(&"").to_string(),
        }),
        "%session-renamed" => Some(ControlModeMessage::SessionRenamed {
            session_id: parts.get(1)?.to_string(),
            new_name: parts.get(2).unwrap_or(&"").to_string(),
        }),
        "%sessions-changed" => Some(ControlModeMessage::SessionsChanged),
        "%session-window-changed" => Some(ControlModeMessage::SessionWindowChanged {
            session_id: parts.get(1)?.to_string(),
            window_id: parts.get(2)?.to_string(),
        }),
        "%client-session-changed" => Some(ControlModeMessage::ClientSessionChanged {
            client: parts.get(1)?.to_string(),
            session_id: parts.get(2)?.to_string(),
            session_name: parts.get(3).unwrap_or(&"").to_string(),
        }),
        "%window-pane-changed" => Some(ControlModeMessage::WindowPaneChanged {
            window_id: parts.get(1)?.to_string(),
            pane_id: parts.get(2)?.to_string(),
        }),
        "%pane-mode-changed" => Some(ControlModeMessage::PaneModeChanged {
            pane_id: parts.get(1)?.to_string(),
        }),
        "%layout-change" => Some(ControlModeMessage::LayoutChange {
            window_id: parts.get(1)?.to_string(),
            layout_string: parts.get(2).unwrap_or(&"").to_string(),
        }),
        "%pause" => Some(ControlModeMessage::Pause {
            pane_id: parts.get(1)?.to_string(),
        }),
        "%continue" => Some(ControlModeMessage::Continue {
            pane_id: parts.get(1)?.to_string(),
        }),
        "%subscription-changed" => Some(ControlModeMessage::SubscriptionChanged {
            name: parts.get(1)?.to_string(),
            value: parts.get(2).unwrap_or(&"").to_string(),
        }),
        "%exit" => {
            let reason = if parts.len() > 1 {
                Some(parts[1..].join(" "))
            } else {
                None
            };
            Some(ControlModeMessage::Exit { reason })
        }
        _ => None, // Unknown notification — ignore gracefully
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_output() {
        let msg = parse_notification("%output %0 hello\\015\\012").unwrap();
        if let ControlModeMessage::Output { pane_id, data } = msg {
            assert_eq!(pane_id, "%0");
            assert_eq!(data, b"hello\r\n");
        } else { panic!("Expected Output"); }
    }

    #[test]
    fn test_parse_window_add() {
        assert_eq!(
            parse_notification("%window-add @1"),
            Some(ControlModeMessage::WindowAdd { window_id: "@1".into() })
        );
    }

    #[test]
    fn test_parse_sessions_changed() {
        assert_eq!(
            parse_notification("%sessions-changed"),
            Some(ControlModeMessage::SessionsChanged)
        );
    }

    #[test]
    fn test_parse_layout_change() {
        let msg = parse_notification("%layout-change @0 177x44,0,0{88x44,0,0,0,88x44,89,0,1}").unwrap();
        if let ControlModeMessage::LayoutChange { window_id, layout_string } = msg {
            assert_eq!(window_id, "@0");
            assert!(layout_string.contains("177x44"));
        } else { panic!("Expected LayoutChange"); }
    }

    #[test]
    fn test_parse_exit() {
        assert_eq!(
            parse_notification("%exit"),
            Some(ControlModeMessage::Exit { reason: None })
        );
        assert_eq!(
            parse_notification("%exit server exited"),
            Some(ControlModeMessage::Exit { reason: Some("server exited".into()) })
        );
    }

    #[test]
    fn test_parse_unknown() {
        assert_eq!(parse_notification("%unknown-event foo"), None);
    }

    #[test]
    fn test_parse_empty() {
        assert_eq!(parse_notification(""), None);
    }

    #[test]
    fn test_parse_non_notification() {
        assert_eq!(parse_notification("just some text"), None);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p psmux -- remote::protocol::tests -v`
Expected: All PASS

- [ ] **Step 3: Commit**

```bash
git add src/remote/protocol.rs
git commit -m "feat(remote): control mode protocol types and notification parser"
```

### Task 12: Control Mode Parser State Machine

**Files:**
- Create: `src/remote/parser.rs`
- Create: `tests/fixtures/tmux_cc_session.txt`
- Test: `tests/control_mode_contracts.rs`

- [ ] **Step 1: Create parser with response block handling**

Create `src/remote/parser.rs`:

```rust
use super::protocol::{parse_notification, ControlModeMessage};

pub struct ControlModeParser {
    state: ParserState,
    block_lines: Vec<String>,
    block_timestamp: u64,
    block_command_id: u64,
    block_flags: u32,
}

enum ParserState {
    Idle,
    InBlock,
}

impl ControlModeParser {
    pub fn new() -> Self {
        Self {
            state: ParserState::Idle,
            block_lines: Vec::new(),
            block_timestamp: 0,
            block_command_id: 0,
            block_flags: 0,
        }
    }

    /// Feed a single line. Returns a parsed message if one is complete.
    pub fn feed_line(&mut self, line: &str) -> Option<ControlModeMessage> {
        let line = line.trim_end();

        match self.state {
            ParserState::Idle => {
                if line.starts_with("%begin ") {
                    // Parse: %begin {timestamp} {command_id} {flags}
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 4 {
                        self.block_timestamp = parts[1].parse().unwrap_or(0);
                        self.block_command_id = parts[2].parse().unwrap_or(0);
                        self.block_flags = parts[3].parse().unwrap_or(0);
                        self.block_lines.clear();
                        self.state = ParserState::InBlock;
                    }
                    None
                } else {
                    // Standalone notification
                    parse_notification(line)
                }
            }
            ParserState::InBlock => {
                if line.starts_with("%end ") || line.starts_with("%error ") {
                    let success = line.starts_with("%end");
                    let msg = ControlModeMessage::Response {
                        timestamp: self.block_timestamp,
                        command_id: self.block_command_id,
                        flags: self.block_flags,
                        body: std::mem::take(&mut self.block_lines),
                        success,
                    };
                    self.state = ParserState::Idle;
                    Some(msg)
                } else {
                    self.block_lines.push(line.to_string());
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_block() {
        let mut parser = ControlModeParser::new();
        assert!(parser.feed_line("%begin 1711234567 42 0").is_none());
        assert!(parser.feed_line("%3").is_none());
        let msg = parser.feed_line("%end 1711234567 42 0").unwrap();
        if let ControlModeMessage::Response { command_id, body, success, .. } = msg {
            assert_eq!(command_id, 42);
            assert!(success);
            assert_eq!(body, vec!["%3"]);
        } else { panic!("Expected Response"); }
    }

    #[test]
    fn test_multiline_response() {
        let mut parser = ControlModeParser::new();
        parser.feed_line("%begin 0 1 0");
        parser.feed_line("0: window-name (1 panes)");
        parser.feed_line("1: vim (1 panes)");
        let msg = parser.feed_line("%end 0 1 0").unwrap();
        if let ControlModeMessage::Response { body, .. } = msg {
            assert_eq!(body.len(), 2);
        } else { panic!(); }
    }

    #[test]
    fn test_error_response() {
        let mut parser = ControlModeParser::new();
        parser.feed_line("%begin 0 5 0");
        parser.feed_line("no such session");
        let msg = parser.feed_line("%error 0 5 0").unwrap();
        if let ControlModeMessage::Response { success, body, .. } = msg {
            assert!(!success);
            assert_eq!(body[0], "no such session");
        } else { panic!(); }
    }

    #[test]
    fn test_interleaved_notifications_and_blocks() {
        let mut parser = ControlModeParser::new();
        // Notification before block
        let n = parser.feed_line("%output %0 hello").unwrap();
        assert!(matches!(n, ControlModeMessage::Output { .. }));

        // Block
        assert!(parser.feed_line("%begin 0 1 0").is_none());
        // Notification CANNOT arrive mid-block in tmux, but if it did:
        assert!(parser.feed_line("response line").is_none());
        let r = parser.feed_line("%end 0 1 0").unwrap();
        assert!(matches!(r, ControlModeMessage::Response { .. }));

        // Notification after block
        let n2 = parser.feed_line("%window-add @1").unwrap();
        assert!(matches!(n2, ControlModeMessage::WindowAdd { .. }));
    }
}
```

- [ ] **Step 2: Create golden transcript fixture**

Create `tests/fixtures/tmux_cc_session.txt` — a minimal recorded tmux -CC session:

```
%begin 1711234567 0 0
%end 1711234567 0 0
%output %0 \033[32muser@vm\033[0m:~$
%window-add @0
%session-changed $0 main
%layout-change @0 80x24,0,0,0
%output %0 ls\015\012
%output %0 file1.txt  file2.txt\015\012
%output %0 \033[32muser@vm\033[0m:~$
%begin 1711234568 1 0
%1
%end 1711234568 1 0
%window-add @1
%layout-change @0 80x24,0,0{40x24,0,0,0,39x24,41,0,1}
%output %1 \033[32muser@vm\033[0m:~$
%window-pane-changed @0 %1
%pane-mode-changed %0
%sessions-changed
```

- [ ] **Step 3: Write golden transcript test**

Create `tests/control_mode_contracts.rs`:

```rust
#[test]
fn test_golden_transcript() {
    let transcript = include_str!("fixtures/tmux_cc_session.txt");
    let mut parser = psmux::remote::parser::ControlModeParser::new();
    let messages: Vec<_> = transcript.lines()
        .filter_map(|line| parser.feed_line(line))
        .collect();

    // Should have parsed notifications + response blocks
    assert!(!messages.is_empty(), "Golden transcript should produce messages");

    // Should contain output
    assert!(messages.iter().any(|m| matches!(m, psmux::remote::protocol::ControlModeMessage::Output { .. })));
    // Should contain window-add
    assert!(messages.iter().any(|m| matches!(m, psmux::remote::protocol::ControlModeMessage::WindowAdd { .. })));
    // Should contain response blocks
    assert!(messages.iter().any(|m| matches!(m, psmux::remote::protocol::ControlModeMessage::Response { success: true, .. })));
    // Should contain layout-change
    assert!(messages.iter().any(|m| matches!(m, psmux::remote::protocol::ControlModeMessage::LayoutChange { .. })));
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test control_mode_contracts -v && cargo test -p psmux -- remote::parser::tests -v`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add src/remote/parser.rs tests/control_mode_contracts.rs tests/fixtures/tmux_cc_session.txt
git commit -m "feat(remote): control mode parser state machine with golden transcript test"
```

### Task 13: Remote Pane Manager

**Files:**
- Create: `src/remote/pane_manager.rs`
- Test: inline

- [ ] **Step 1: Implement RemotePaneManager**

```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use super::protocol::ControlModeMessage;

pub struct RemotePaneManager {
    /// pane_id -> VT100 parser for rendering
    panes: HashMap<String, Arc<Mutex<vt100::Parser>>>,
    /// window_id -> list of pane_ids
    windows: HashMap<String, Vec<String>>,
    /// Currently active pane
    active_pane: Option<String>,
    /// Terminal size
    cols: u16,
    rows: u16,
}

impl RemotePaneManager {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            panes: HashMap::new(),
            windows: HashMap::new(),
            active_pane: None,
            cols,
            rows,
        }
    }

    /// Process a control mode message, updating internal state.
    pub fn handle_message(&mut self, msg: ControlModeMessage) {
        match msg {
            ControlModeMessage::Output { pane_id, data } => {
                if let Some(parser) = self.panes.get(&pane_id) {
                    if let Ok(mut p) = parser.lock() {
                        p.process(&data);
                    }
                }
            }
            ControlModeMessage::WindowAdd { window_id } => {
                self.windows.entry(window_id).or_default();
            }
            ControlModeMessage::WindowClose { window_id } => {
                if let Some(pane_ids) = self.windows.remove(&window_id) {
                    for pid in pane_ids {
                        self.panes.remove(&pid);
                    }
                }
            }
            ControlModeMessage::LayoutChange { window_id, layout_string } => {
                // Parse tmux layout string to discover pane IDs and geometry
                let pane_ids = parse_layout_pane_ids(&layout_string);
                for pid in &pane_ids {
                    if !self.panes.contains_key(pid) {
                        self.panes.insert(
                            pid.clone(),
                            Arc::new(Mutex::new(vt100::Parser::new(self.rows, self.cols, 0))),
                        );
                    }
                }
                self.windows.insert(window_id, pane_ids);
            }
            ControlModeMessage::WindowPaneChanged { pane_id, .. } => {
                self.active_pane = Some(pane_id);
            }
            ControlModeMessage::Exit { .. } => {
                // Session ended — will be handled by SshTransport
            }
            _ => {} // Other notifications: log but don't act
        }
    }

    pub fn get_active_pane(&self) -> Option<&Arc<Mutex<vt100::Parser>>> {
        self.active_pane.as_ref().and_then(|id| self.panes.get(id))
    }

    pub fn get_pane(&self, pane_id: &str) -> Option<&Arc<Mutex<vt100::Parser>>> {
        self.panes.get(pane_id)
    }

    pub fn pane_ids(&self) -> Vec<String> {
        self.panes.keys().cloned().collect()
    }
}

/// Extract pane IDs from a tmux layout string.
///
/// tmux layout format: leaf nodes are `WxH,X,Y,PANE_ID` (4 comma fields).
/// Split nodes are `WxH,X,Y{...}` (horizontal) or `WxH,X,Y[...]` (vertical).
/// The pane ID is the 4th comma-separated field in a leaf node — it appears
/// just before `}`, `]`, `,` (next sibling), or end-of-string.
///
/// Example: "80x24,0,0{40x24,0,0,0,39x24,41,0,1}"
///   - Leaf 1: 40x24,0,0,0 → pane ID 0
///   - Leaf 2: 39x24,41,0,1 → pane ID 1
fn parse_layout_pane_ids(layout: &str) -> Vec<String> {
    let mut ids = Vec::new();
    // Use regex to find leaf nodes: WxH,X,Y,PANE_ID where PANE_ID is
    // followed by } or ] or , (next sibling) or end-of-string
    let re = regex::Regex::new(r"(\d+)x(\d+),(\d+),(\d+),(\d+)").unwrap();
    for cap in re.captures_iter(layout) {
        if let Some(pane_id_match) = cap.get(5) {
            let pane_id = format!("%{}", pane_id_match.as_str());
            if !ids.contains(&pane_id) {
                ids.push(pane_id);
            }
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_output_routes_to_pane() {
        let mut mgr = RemotePaneManager::new(80, 24);
        // Add a pane
        mgr.panes.insert("%0".into(), Arc::new(Mutex::new(vt100::Parser::new(24, 80, 0))));

        mgr.handle_message(ControlModeMessage::Output {
            pane_id: "%0".into(),
            data: b"hello".to_vec(),
        });

        let parser = mgr.panes.get("%0").unwrap().lock().unwrap();
        let screen = parser.screen();
        let row = screen.contents_between(0, 0, 0, 5);
        assert!(row.contains("hello"));
    }

    #[test]
    fn test_window_add_close() {
        let mut mgr = RemotePaneManager::new(80, 24);
        mgr.handle_message(ControlModeMessage::WindowAdd { window_id: "@0".into() });
        assert!(mgr.windows.contains_key("@0"));

        mgr.handle_message(ControlModeMessage::WindowClose { window_id: "@0".into() });
        assert!(!mgr.windows.contains_key("@0"));
    }

    #[test]
    fn test_layout_change_creates_panes() {
        let mut mgr = RemotePaneManager::new(80, 24);
        mgr.handle_message(ControlModeMessage::LayoutChange {
            window_id: "@0".into(),
            layout_string: "80x24,0,0{40x24,0,0,0,39x24,41,0,1}".into(),
        });
        assert!(mgr.panes.contains_key("%0"));
        assert!(mgr.panes.contains_key("%1"));
        // Should NOT contain phantom IDs like %80 or %24
        assert!(!mgr.panes.contains_key("%80"));
        assert!(!mgr.panes.contains_key("%24"));
        assert_eq!(mgr.panes.len(), 2);
    }

    #[test]
    fn test_parse_layout_pane_ids() {
        // Single pane
        assert_eq!(parse_layout_pane_ids("80x24,0,0,0"), vec!["%0"]);
        // Two panes horizontal split
        assert_eq!(
            parse_layout_pane_ids("80x24,0,0{40x24,0,0,0,39x24,41,0,1}"),
            vec!["%0", "%1"]
        );
        // Three panes
        assert_eq!(
            parse_layout_pane_ids("177x44,0,0{88x44,0,0,0,88x44,89,0[88x22,89,0,1,88x21,89,23,2]}"),
            vec!["%0", "%1", "%2"]
        );
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p psmux -- remote::pane_manager::tests -v`
Expected: All PASS

- [ ] **Step 3: Commit**

```bash
git add src/remote/pane_manager.rs
git commit -m "feat(remote): RemotePaneManager maps control mode events to VT100 parsers"
```

### Task 14: SSH Transport

**Files:**
- Create: `src/remote/ssh.rs`
- Test: inline

- [ ] **Step 1: Implement SshTransport**

```rust
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

pub struct SshTransport {
    child: Child,
    stdin: Box<dyn Write + Send>,
    reader: BufReader<Box<dyn io::Read + Send>>,
}

impl SshTransport {
    pub fn connect(
        ssh_target: &str,
        session: &str,
        attach: bool,
        ssh_opts: Option<&str>,
    ) -> io::Result<Self> {
        let tmux_cmd = if attach {
            format!("tmux -CC attach -t {}", session)
        } else {
            format!("tmux -CC new-session -s {}", session)
        };

        let mut cmd = Command::new("ssh");
        if let Some(opts) = ssh_opts {
            for opt in opts.split_whitespace() {
                cmd.arg(opt);
            }
        }
        cmd.arg(ssh_target)
            .arg(tmux_cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdin = Box::new(child.stdin.take().unwrap());
        let stdout: Box<dyn io::Read + Send> = Box::new(child.stdout.take().unwrap());
        let reader = BufReader::new(stdout);

        Ok(Self { child, stdin, reader })
    }

    pub fn send_command(&mut self, cmd: &str) -> io::Result<()> {
        writeln!(self.stdin, "{}", cmd)?;
        self.stdin.flush()
    }

    pub fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        self.reader.read_line(buf)
    }

    pub fn is_alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }

    pub fn disconnect(mut self) {
        let _ = self.stdin.write_all(b"\n"); // Empty line = detach in control mode
        let _ = self.child.wait();
    }
}
```

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 3: Commit**

```bash
git add src/remote/ssh.rs
git commit -m "feat(remote): SshTransport for tmux control mode connection"
```

### Task 15: Wire Remote Client + CLI Commands

**Files:**
- Create: `src/remote/mod.rs`
- Modify: `src/main.rs:248` — add subcommands
- Modify: `src/client.rs` — add `run_remote_tmux()`

- [ ] **Step 1: Create remote module root**

Create `src/remote/mod.rs`:

```rust
pub mod octal;
pub mod protocol;
pub mod parser;
pub mod pane_manager;
pub mod ssh;

use parser::ControlModeParser;
use pane_manager::RemotePaneManager;
use ssh::SshTransport;

/// Main entry point for remote tmux session rendering.
pub fn run_remote_tmux(
    ssh_target: &str,
    session: Option<&str>,
    ssh_opts: Option<&str>,
) -> std::io::Result<()> {
    let session_name = session.unwrap_or("default");

    // Try attach first, fall back to new-session
    let mut transport = SshTransport::connect(ssh_target, session_name, true, ssh_opts)
        .or_else(|_| SshTransport::connect(ssh_target, session_name, false, ssh_opts))?;

    let (cols, rows) = crossterm::terminal::size()?;
    transport.send_command(&format!("refresh-client -C {}x{}", cols, rows))?;

    let mut parser = ControlModeParser::new();
    let mut manager = RemotePaneManager::new(cols, rows);

    // Read loop — feed lines from SSH stdout to parser
    let mut line = String::new();
    loop {
        line.clear();
        match transport.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {
                if let Some(msg) = parser.feed_line(&line) {
                    if matches!(msg, protocol::ControlModeMessage::Exit { .. }) {
                        break;
                    }
                    manager.handle_message(msg);
                }
            }
            Err(e) => {
                eprintln!("SSH read error: {}", e);
                break;
            }
        }
    }

    transport.disconnect();
    Ok(())
}
```

Note: This is a minimal wiring — the full interactive rendering (keyboard input, ratatui rendering) will be integrated in a follow-up task by extending `src/client.rs:run_remote()` to support the remote pane manager.

- [ ] **Step 2: Add CLI subcommands to main.rs**

In `src/main.rs`, add to the match block (around line 248):

```rust
"attach-remote" => {
    let ssh_target = cmd_args.get(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Usage: psmux attach-remote <user@host> [-t session]"))?;
    let session = extract_arg(&cmd_args, "-t");
    let ssh_opts = extract_arg(&cmd_args, "--ssh-opts");
    return crate::remote::run_remote_tmux(ssh_target, session.as_deref(), ssh_opts.as_deref());
}
"new-session-remote" => {
    let ssh_target = cmd_args.get(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Usage: psmux new-session-remote <user@host> -s <name>"))?;
    let session = extract_arg(&cmd_args, "-s");
    let ssh_opts = extract_arg(&cmd_args, "--ssh-opts");
    return crate::remote::run_remote_tmux(ssh_target, session.as_deref(), ssh_opts.as_deref());
}
"list-sessions-remote" => {
    let ssh_target = cmd_args.get(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Usage: psmux list-sessions-remote <user@host>"))?;
    // SSH in, run tmux list-sessions, print output
    let output = std::process::Command::new("ssh")
        .arg(ssh_target)
        .arg("tmux list-sessions")
        .output()?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    return Ok(());
}
```

Add `mod remote;` to the module declarations.

- [ ] **Step 3: Build and basic test**

Run: `cargo build && cargo run -- list-sessions-remote --help 2>&1 | head -5`
Expected: Build succeeds, command is recognized

- [ ] **Step 4: Commit**

```bash
git add src/remote/mod.rs src/main.rs
git commit -m "feat(remote): wire attach-remote and new-session-remote CLI commands"
```

### Task 16: Interactive Rendering for Remote Sessions

**Files:**
- Modify: `src/client.rs` — add `run_remote_tmux()` with full keyboard + rendering
- Modify: `src/remote/mod.rs` — upgrade `run_remote_tmux` to pass to client

This is the largest single task — it integrates the SSH transport + parser + pane manager with the existing ratatui rendering loop. The approach: fork the existing `run_remote()` function pattern but substitute TCP frame reception with SSH control mode line parsing.

- [ ] **Step 1: Implement the interactive loop**

In `src/remote/mod.rs`, replace the basic `run_remote_tmux` with a version that enters the client's rendering loop. The key changes from the local path:
- Instead of receiving JSON dump-state frames, we receive `%output` notifications
- Instead of sending TCP commands, we write tmux commands to SSH stdin
- Keyboard events → `send-keys -t %{active} {key}` over SSH
- Terminal resize → `refresh-client -C {w}x{h}` over SSH
- Prefix key (Ctrl+b) handled locally as before

This step reuses `render_layout()`, `handle_copy_mode()`, `handle_prefix()`, `draw_status_bar()` from `src/client.rs`.

- [ ] **Step 2: Manual integration test**

Requires WSL or a Linux VM with tmux installed:

```bash
# Start tmux on WSL
wsl tmux new-session -d -s test

# Connect from psmux
cargo run -- attach-remote localhost -t test
```

Expected: Remote tmux session renders in psmux window. Keystrokes work. Ctrl+b d detaches.

- [ ] **Step 3: Commit**

```bash
git add src/client.rs src/remote/mod.rs
git commit -m "feat(remote): interactive rendering for remote tmux sessions via control mode"
```

---

## Final Tasks

### Task 17: Update CI and Documentation

**Files:**
- Modify: `.github/workflows/ci.yml` (if exists)
- Modify: `CLAUDE.md`

- [ ] **Step 1: Add boundary contract test stage to CI**

```yaml
boundary-contracts:
  runs-on: windows-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - run: cargo test --test backend_contracts
    - run: cargo test --test control_mode_contracts
    - run: cargo test --test passthrough_tests
```

- [ ] **Step 2: Update CLAUDE.md with new commands and features**

Add to the tmux Compatibility Status section:
- `attach-remote`, `new-session-remote`, `list-sessions-remote`
- CustomPaneBackend via `CLAUDE_PANE_BACKEND_SOCKET`
- DCS passthrough via `set -g allow-passthrough on`

- [ ] **Step 3: Run full check suite**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: All pass, zero warnings

- [ ] **Step 4: Commit**

```bash
git add .github/ CLAUDE.md
git commit -m "docs: update CLAUDE.md and CI for remote, backend, and passthrough features"
```

---

## Task Dependency Graph

```
Track C (DCS Passthrough):
  Task 1 → Task 2 → Task 3 → Task 4

Track B (CustomPaneBackend):
  Task 5 → Task 6 → Task 7 → Task 8 → Task 9

Track A (Control Mode Client):
  Task 10 → Task 11 → Task 12 → Task 13 → Task 14 → Task 15 → Task 16

All tracks → Task 17 (CI + docs)
```

Tracks are independent — can be worked on in any order or in parallel by different agents.

## Estimated Effort

| Track | Tasks | Estimated LOC | Estimated Time |
|-------|-------|---------------|----------------|
| C: DCS Passthrough | 1-4 | ~100 | 1-2 sessions |
| B: CustomPaneBackend | 5-9 | ~350 | 2-3 sessions |
| A: Control Mode | 10-16 | ~900 | 3-5 sessions |
| Final | 17 | ~50 | 1 session |
| **Total** | **17 tasks** | **~1400 LOC** | **7-11 sessions** |
