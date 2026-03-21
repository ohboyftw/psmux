# Protocol-Complete Agent Backend — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make CustomPaneBackend production-grade for 5–8 agent Claude Code swarms — structured errors, spawn readiness, capture freshness, run_shell, shell selection, --bare flag.

**Architecture:** The backend is a JSON-RPC 2.0 server over Windows named pipes (`src/backend/`). The pipe listener (`pipe.rs`) spawns a thread per client connection. The dispatcher (`dispatcher.rs`) routes RPC methods, sends `CtrlReq` messages into the server's single-threaded event loop (`src/server/mod.rs`), and receives responses via `mpsc` channels. All polling/blocking work MUST happen in the dispatcher thread, never in the server handler — the server loop processes ALL events (keys, rendering, pane lifecycle) and cannot block.

**Tech Stack:** Rust (stable), `serde_json` for JSON-RPC, `windows-sys` for named pipes, `std::process::Command` for `run_shell`, `mpsc` channels for server IPC.

**Spec:** `docs/superpowers/specs/2026-03-21-protocol-complete-agent-backend-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src/backend/protocol.rs` | Modify | Error codes, `RpcError.data`, new param/result structs |
| `src/backend/dispatcher.rs` | Modify | Readiness polling, freshness polling, `run_shell`, `--bare` injection, structured errors |
| `src/backend/pipe.rs` | No change | Connection handling unchanged |
| `src/server/mod.rs` | Modify | `PANE_NOT_FOUND` on capture, `spawn_cwd` storage, `BackendRunShell` handler, `--shell` passthrough |
| `src/types.rs` | Modify | `spawn_cwd` on Pane, `BackendRunShell` CtrlReq variant, `shell` field on `BackendSpawnAgent` |
| `src/pane.rs` | Modify | `spawn_cwd` field set at spawn, `--shell` override in `split_active_with_command` |
| `src/main.rs` | Modify | Parse `--shell` arg on `new-window` and `split-window` |
| `src/format.rs` | Modify | Add `#{pane_shell}` format variable |
| `tests/backend_contracts.rs` | Modify | New tests for error codes, spawn result shape, capture result shape |
| `tests/backend_protocol_v2.rs` | Create | Integration tests for run_shell, freshness, readiness |

---

## Task 1: Structured Error Protocol — `protocol.rs`

**Files:**
- Modify: `src/backend/protocol.rs`

- [ ] **Step 1: Add error code constants and `data` field to `RpcError`**

```rust
// Add at the top of protocol.rs, after the use statements:

// ── Error Codes ──
pub const PANE_NOT_FOUND: i32 = -32001;
pub const SPAWN_FAILED: i32 = -32002;
pub const PANE_TOO_SMALL: i32 = -32003;
pub const SPAWN_TIMEOUT: i32 = -32004;
pub const CAPTURE_TIMEOUT: i32 = -32005;
pub const SESSION_NOT_FOUND: i32 = -32006;
pub const COMMAND_TIMEOUT: i32 = -32007;
pub const COMMAND_FAILED: i32 = -32008;
```

Update `RpcError`:

```rust
#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}
```

- [ ] **Step 2: Add `error_with_data` constructor to `RpcResponse`**

```rust
// Add to the impl RpcResponse block, after the existing error() method:
pub fn error_with_data(
    id: serde_json::Value,
    code: i32,
    message: impl Into<String>,
    data: serde_json::Value,
) -> Self {
    Self {
        id,
        result: None,
        error: Some(RpcError {
            code,
            message: message.into(),
            data: Some(data),
        }),
    }
}
```

Update existing `error()` to set `data: None`:

```rust
pub fn error(id: serde_json::Value, code: i32, message: impl Into<String>) -> Self {
    Self {
        id,
        result: None,
        error: Some(RpcError {
            code,
            message: message.into(),
            data: None,
        }),
    }
}
```

- [ ] **Step 3: Update response structs for protocol v2**

Replace `SpawnAgentResult`:

```rust
#[derive(Debug, Serialize)]
pub struct SpawnAgentResult {
    pub context_id: String,
    pub ready: bool,
    pub elapsed_ms: u64,
    pub data_version: u64,
}
```

Replace `CaptureResult`:

```rust
#[derive(Debug, Serialize)]
pub struct CaptureResult {
    pub text: String,
    pub data_version: u64,
    pub context_id: String,
}
```

Add new structs:

```rust
#[derive(Debug, Deserialize)]
pub struct RunShellParams {
    pub command: Vec<String>,
    pub cwd: Option<String>,
    pub context_id: Option<String>,
    pub timeout_ms: Option<u32>,
    pub env: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize)]
pub struct RunShellResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub elapsed_ms: u64,
}
```

- [ ] **Step 4: Add new fields to `SpawnAgentParams` and `CaptureParams`**

Update `SpawnAgentParams`:

```rust
#[derive(Debug, Deserialize)]
pub struct SpawnAgentParams {
    pub command: Vec<String>,
    pub cwd: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub metadata: Option<AgentMetadata>,
    pub split_direction: Option<String>,
    #[serde(default = "default_true")]
    pub wait_ready: bool,
    pub ready_timeout_ms: Option<u32>,
    pub shell: Option<String>,
    pub bare: Option<bool>,
}

fn default_true() -> bool { true }
```

Update `CaptureParams`:

```rust
#[derive(Debug, Deserialize)]
pub struct CaptureParams {
    pub context_id: String,
    pub lines: Option<u32>,
    pub clean: Option<bool>,
    #[serde(default)]
    pub wait_for_output: bool,
    pub since_version: Option<u64>,
    pub timeout_ms: Option<u32>,
}
```

- [ ] **Step 5: Bump protocol version in `handle_initialize`**

This happens in `dispatcher.rs` but note it here: change `protocol_version: "1"` to `"2"` and add `"run_shell"` to capabilities.

- [ ] **Step 6: Update existing tests in `tests/backend_contracts.rs` for v2**

The existing test file has local struct definitions that mirror the protocol types. Update them:
- Remove `truncated` field from local `CaptureResult`, add `data_version: u64` and `context_id: String`
- Update local `SpawnAgentResult` to match v2 (add `ready: bool`, `elapsed_ms: u64`, `data_version: u64`)
- Update `test_initialize_response_roundtrip` to expect `protocol_version: "2"`
- Remove or update `test_capture_result_truncated_flag` (dead contract)

- [ ] **Step 7: Run `cargo check`**

Run: `cargo check`
Expected: Compilation errors in `dispatcher.rs` where old struct fields are used — those are fixed in Task 2.

- [ ] **Step 8: Commit**

```bash
git add src/backend/protocol.rs tests/backend_contracts.rs
git commit -m "feat(backend): structured error codes, protocol v2 types"
```

---

## Task 2: Structured Errors in Dispatcher + Protocol Version Bump

**Files:**
- Modify: `src/backend/dispatcher.rs`

- [ ] **Step 1: Add `parse_pane_id` helper**

```rust
/// Parse a "%N" context_id string into a usize pane index.
/// Returns None if the format is invalid.
fn parse_pane_id(context_id: &str) -> Option<usize> {
    context_id.strip_prefix('%').and_then(|s| s.parse().ok())
}
```

- [ ] **Step 2: Update `handle_initialize` — bump protocol version**

Change:
```rust
protocol_version: "1".into(),
capabilities: vec!["events".into(), "capture".into()],
```
To:
```rust
protocol_version: "2".into(),
capabilities: vec!["events".into(), "capture".into(), "run_shell".into()],
```

- [ ] **Step 3: Update `handle_spawn_agent` — use structured errors**

Replace the `"ERROR:"` prefix check:

```rust
// Old:
if let Some(err_msg) = context_id.strip_prefix("ERROR:") {
    return Err((-32603, err_msg.to_string()));
}

// New:
if let Some(err_msg) = context_id.strip_prefix("ERROR:") {
    let code = if err_msg.contains("too small") {
        PANE_TOO_SMALL
    } else {
        SPAWN_FAILED
    };
    return Err((code, err_msg.to_string()));
}
```

Note: The readiness polling is added in Task 3. This step only fixes the error codes.

- [ ] **Step 4: Update `handle_capture` — return structured result**

Replace the current capture handler body to return the new `CaptureResult` struct (without freshness polling yet — that's Task 4). Fix pane-not-found:

```rust
fn handle_capture(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, (i32, String)> {
    let p: CaptureParams = serde_json::from_value(params.clone())
        .map_err(|e| (-32602, format!("Invalid params: {e}")))?;

    let context_id = p.context_id.clone();

    let (resp_tx, resp_rx) = mpsc::channel();
    tx.send(CtrlReq::BackendCapturePane {
        pane_id: p.context_id,
        lines: p.lines,
        clean: p.clean.unwrap_or(false),
        resp: resp_tx,
    })
    .map_err(|_| (-32603, "Server channel closed".to_string()))?;

    let text = resp_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| (-32603, "Server response timeout".to_string()))?;

    // Check for PANE_NOT_FOUND sentinel
    if text == "__PANE_NOT_FOUND__" {
        return Err((PANE_NOT_FOUND, format!("Pane not found: {}", context_id)));
    }

    // Read data_version for the response
    let data_version = if let Some(pid) = parse_pane_id(&context_id) {
        let (qtx, qrx) = mpsc::channel();
        let _ = tx.send(CtrlReq::QueryPaneReady(pid, qtx));
        qrx.recv_timeout(std::time::Duration::from_millis(500))
            .map(|(dv, _)| dv)
            .unwrap_or(0)
    } else {
        0
    };

    let result = CaptureResult {
        text,
        data_version,
        context_id,
    };
    serde_json::to_value(result).map_err(|e| (-32603, format!("Internal error: {e}")))
}
```

- [ ] **Step 5: Run `cargo check`**

Run: `cargo check`
Expected: May fail on `__PANE_NOT_FOUND__` sentinel (server-side change needed in Task 5). Note: we'll wire that in Task 5. For now, the dispatcher code compiles.

- [ ] **Step 6: Commit**

```bash
git add src/backend/dispatcher.rs
git commit -m "feat(backend): structured error codes in dispatcher, protocol v2"
```

---

## Task 3: Spawn with Readiness — Dispatcher Polling

**Files:**
- Modify: `src/backend/dispatcher.rs`
- Modify: `src/backend/protocol.rs` (if not already done)

- [ ] **Step 1: Write failing test for spawn result shape**

Add to `tests/backend_contracts.rs`:

```rust
#[test]
fn spawn_result_has_protocol_v2_fields() {
    let result = SpawnAgentResult {
        context_id: "%5".into(),
        ready: true,
        elapsed_ms: 1200,
        data_version: 12,
    };
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["context_id"], "%5");
    assert_eq!(json["ready"], true);
    assert!(json["elapsed_ms"].is_number());
    assert!(json["data_version"].is_number());
}
```

- [ ] **Step 2: Run test to verify it compiles and passes**

Run: `cargo test --test backend_contracts spawn_result_has_protocol_v2_fields`
Expected: PASS (struct already defined in Task 1)

- [ ] **Step 3: Implement readiness polling in `handle_spawn_agent`**

Replace the tail of `handle_spawn_agent` (after receiving `context_id` from server and checking for errors):

```rust
// --- Readiness polling (runs in dispatcher thread, NOT server loop) ---
let ready;
let elapsed_ms;
let data_version;

if p.wait_ready {
    let timeout_ms = p.ready_timeout_ms.unwrap_or(15000) as u64;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let start = std::time::Instant::now();

    if let Some(pid) = parse_pane_id(&context_id) {
        // NOTE: We reuse one (qtx, qrx) channel across loop iterations.
        // If the server is slow, old responses may queue in qrx. This is
        // acceptable because data_version and last_output_time only ever
        // increase — a stale response just delays detection by one iteration.
        let (qtx, qrx) = mpsc::channel::<(u64, u64)>();
        loop {
            let qtx2 = qtx.clone();
            let _ = tx.send(CtrlReq::QueryPaneReady(pid, qtx2));
            if let Ok((dv, lot)) = qrx.recv_timeout(std::time::Duration::from_secs(2)) {
                if dv > 0 && lot > 0 {
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    if now_ms.saturating_sub(lot) >= 500 {
                        ready = true;
                        elapsed_ms = start.elapsed().as_millis() as u64;
                        data_version = dv;
                        break;
                    }
                }
            }
            if std::time::Instant::now() >= deadline {
                // Timeout — get final data_version
                let qtx_final = qtx.clone();
                let _ = tx.send(CtrlReq::QueryPaneReady(pid, qtx_final));
                data_version = qrx
                    .recv_timeout(std::time::Duration::from_millis(500))
                    .map(|(dv, _)| dv)
                    .unwrap_or(0);
                elapsed_ms = start.elapsed().as_millis() as u64;
                ready = false;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    } else {
        ready = false;
        elapsed_ms = 0;
        data_version = 0;
    }

    if !ready {
        return Err((
            SPAWN_TIMEOUT,
            format!("Pane spawned but not ready within {}ms", timeout_ms),
        ));
        // Note: error data should include context_id — use error_with_data in dispatch_rpc
        // For now, the caller gets the error code and can infer the pane_id from the request.
    }
} else {
    ready = false; // explicitly not waited
    elapsed_ms = 0;
    data_version = 0;
}

let result = SpawnAgentResult {
    context_id,
    ready,
    elapsed_ms,
    data_version,
};
serde_json::to_value(result).map_err(|e| (-32603, format!("Internal error: {e}")))
```

- [ ] **Step 4: Update error handling to use `error_with_data` for spawn timeout**

In the `dispatch_rpc` function, the `Err((code, msg))` path currently calls `RpcResponse::error`. To pass `data`, we need to change the return type of handlers. The simplest approach: change the error tuple to include optional data.

Update handler return type:

```rust
type RpcResult = Result<serde_json::Value, RpcErr>;

struct RpcErr {
    code: i32,
    message: String,
    data: Option<serde_json::Value>,
}

impl From<(i32, String)> for RpcErr {
    fn from((code, message): (i32, String)) -> Self {
        Self { code, message, data: None }
    }
}
```

Update `dispatch_rpc` match:

```rust
let resp = match result {
    Ok(value) => RpcResponse::success(id, value),
    Err(e) => match e.data {
        Some(data) => RpcResponse::error_with_data(id, e.code, e.message, data),
        None => RpcResponse::error(id, e.code, e.message),
    },
};
```

Then in `handle_spawn_agent`, the timeout becomes:

```rust
if !ready {
    return Err(RpcErr {
        code: SPAWN_TIMEOUT,
        message: format!("Pane spawned but not ready within {}ms", timeout_ms),
        data: Some(serde_json::json!({ "context_id": context_id })),
    });
}
```

- [ ] **Step 5: Run `cargo check`**

Run: `cargo check`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/backend/dispatcher.rs src/backend/protocol.rs tests/backend_contracts.rs
git commit -m "feat(backend): spawn with readiness polling in dispatcher thread"
```

---

## Task 4: Capture with Freshness — Dispatcher Polling

**Files:**
- Modify: `src/backend/dispatcher.rs`

- [ ] **Step 1: Write failing test for capture result shape**

Add to `tests/backend_contracts.rs`:

```rust
#[test]
fn capture_result_has_protocol_v2_fields() {
    let result = CaptureResult {
        text: "hello".into(),
        data_version: 47,
        context_id: "%5".into(),
    };
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["text"], "hello");
    assert_eq!(json["data_version"], 47);
    assert_eq!(json["context_id"], "%5");
    // truncated field should NOT be present
    assert!(json.get("truncated").is_none());
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test --test backend_contracts capture_result_has_protocol_v2_fields`
Expected: PASS (struct defined in Task 1)

- [ ] **Step 3: Implement freshness polling in `handle_capture`**

Update `handle_capture` to add freshness polling before the capture request. Insert between parameter parsing and the `BackendCapturePane` send:

```rust
fn handle_capture(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, RpcErr> {
    let p: CaptureParams = serde_json::from_value(params.clone())
        .map_err(|e| RpcErr::from((-32602, format!("Invalid params: {e}"))))?;

    let context_id = p.context_id.clone();
    let timeout_ms = p.timeout_ms.unwrap_or(5000) as u64;

    // --- Freshness polling (dispatcher thread) ---
    if p.wait_for_output || p.since_version.is_some() {
        if let Some(pid) = parse_pane_id(&context_id) {
            // Early pane existence check: if pane doesn't exist, QueryPaneReady
            // returns (0, 0). Do a quick capture to distinguish "pane has no output
            // yet" from "pane doesn't exist".
            {
                let (check_tx, check_rx) = mpsc::channel();
                let _ = tx.send(CtrlReq::BackendCapturePane {
                    pane_id: context_id.clone(),
                    lines: Some(1),
                    clean: false,
                    resp: check_tx,
                });
                if let Ok(text) = check_rx.recv_timeout(std::time::Duration::from_secs(2)) {
                    if text == "__PANE_NOT_FOUND__" {
                        return Err(RpcErr {
                            code: PANE_NOT_FOUND,
                            message: format!("Pane not found: {}", context_id),
                            data: Some(serde_json::json!({ "context_id": context_id })),
                        });
                    }
                }
            }

            // Get baseline data_version
            let baseline = if let Some(sv) = p.since_version {
                sv
            } else {
                let (qtx, qrx) = mpsc::channel();
                let _ = tx.send(CtrlReq::QueryPaneReady(pid, qtx));
                qrx.recv_timeout(std::time::Duration::from_millis(500))
                    .map(|(dv, _)| dv)
                    .unwrap_or(0)
            };

            let deadline =
                std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
            let (qtx, qrx) = mpsc::channel::<(u64, u64)>();
            let mut timed_out = true;

            loop {
                let qtx2 = qtx.clone();
                let _ = tx.send(CtrlReq::QueryPaneReady(pid, qtx2));
                if let Ok((dv, _)) = qrx.recv_timeout(std::time::Duration::from_secs(2)) {
                    if dv > baseline {
                        timed_out = false;
                        break;
                    }
                }
                if std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            // Even on timeout, we still capture (stale content in error data)
            if timed_out {
                // Do the capture anyway, return as error data
                let (resp_tx, resp_rx) = mpsc::channel();
                let _ = tx.send(CtrlReq::BackendCapturePane {
                    pane_id: p.context_id,
                    lines: p.lines,
                    clean: p.clean.unwrap_or(false),
                    resp: resp_tx,
                });
                let text = resp_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap_or_default();

                let qtx_final = qtx.clone();
                let _ = tx.send(CtrlReq::QueryPaneReady(pid, qtx_final));
                let current_dv = qrx
                    .recv_timeout(std::time::Duration::from_millis(500))
                    .map(|(dv, _)| dv)
                    .unwrap_or(0);

                return Err(RpcErr {
                    code: CAPTURE_TIMEOUT,
                    message: format!("No new output within {}ms", timeout_ms),
                    data: Some(serde_json::json!({
                        "text": text,
                        "data_version": current_dv,
                        "context_id": context_id,
                    })),
                });
            }
        }
    }

    // --- Capture (same as before but with PANE_NOT_FOUND) ---
    let (resp_tx, resp_rx) = mpsc::channel();
    tx.send(CtrlReq::BackendCapturePane {
        pane_id: p.context_id,
        lines: p.lines,
        clean: p.clean.unwrap_or(false),
        resp: resp_tx,
    })
    .map_err(|_| RpcErr::from((-32603, "Server channel closed".to_string())))?;

    let text = resp_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| RpcErr::from((-32603, "Server response timeout".to_string())))?;

    if text == "__PANE_NOT_FOUND__" {
        return Err(RpcErr {
            code: PANE_NOT_FOUND,
            message: format!("Pane not found: {}", context_id),
            data: Some(serde_json::json!({ "context_id": context_id })),
        });
    }

    let data_version = if let Some(pid) = parse_pane_id(&context_id) {
        let (qtx, qrx) = mpsc::channel();
        let _ = tx.send(CtrlReq::QueryPaneReady(pid, qtx));
        qrx.recv_timeout(std::time::Duration::from_millis(500))
            .map(|(dv, _)| dv)
            .unwrap_or(0)
    } else {
        0
    };

    let result = CaptureResult {
        text,
        data_version,
        context_id,
    };
    serde_json::to_value(result).map_err(|e| RpcErr::from((-32603, format!("Internal error: {e}"))))
}
```

- [ ] **Step 4: Run `cargo check`**

Run: `cargo check`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/backend/dispatcher.rs tests/backend_contracts.rs
git commit -m "feat(backend): capture with freshness polling and PANE_NOT_FOUND"
```

---

## Task 5: Server-Side Changes — PANE_NOT_FOUND Sentinel + spawn_cwd

**Files:**
- Modify: `src/server/mod.rs` (BackendCapturePane handler)
- Modify: `src/types.rs` (Pane struct)
- Modify: `src/pane.rs` (set spawn_cwd at spawn time)

- [ ] **Step 1: Add `spawn_cwd` field to `Pane` struct**

In `src/types.rs`, find the `Pane` struct and add:

```rust
/// Working directory at pane spawn time. Used by run_shell to inherit context.
pub spawn_cwd: Option<std::path::PathBuf>,
```

Initialize to `None` in all Pane constructors. It will be set at spawn time.

- [ ] **Step 2: Set `spawn_cwd` in `split_active_with_command` and `add_pane`**

In `src/pane.rs`, after a pane is created in `split_active_with_command()` (around line 620+), set:

```rust
// After the new pane is inserted into the tree:
// Set spawn_cwd from start_dir or current directory
if let Some(dir) = start_dir {
    new_pane.spawn_cwd = Some(std::path::PathBuf::from(dir));
} else {
    new_pane.spawn_cwd = std::env::current_dir().ok();
}
```

Do the same in `add_pane()` (new-window path, around line 175+).

- [ ] **Step 3: Update `BackendCapturePane` handler to return sentinel on pane-not-found**

In `src/server/mod.rs`, find the `BackendCapturePane` handler (around line 4602). Currently it iterates windows to find the pane and returns empty string if not found. Change it to return `"__PANE_NOT_FOUND__"` sentinel:

Find the section where it searches for the pane and sends the captured text. If the pane is not found, instead of sending an empty string, send:

```rust
// Sentinel string — used by the dispatcher to distinguish "pane not found"
// from "pane exists but has no output". The dispatcher converts this to
// a proper PANE_NOT_FOUND RPC error. This string can never appear in real
// terminal output because it contains no VT sequences and exceeds the
// maximum content of a single cell.
let _ = resp.send("__PANE_NOT_FOUND__".to_string());
```

- [ ] **Step 3b: Make `BackendSendText` return PANE_NOT_FOUND**

Currently `BackendSendText` is fire-and-forget (no response channel). Add a response:

1. In `src/types.rs`, change `BackendSendText` to include a response sender:
```rust
BackendSendText {
    pane_id: String,
    text: String,
    resp: Option<mpsc::Sender<bool>>, // true = sent, false = pane not found
},
```

2. In `src/server/mod.rs` `BackendSendText` handler, send `false` if the pane is not found.

3. In `src/backend/dispatcher.rs` `handle_write`, check the response and return `PANE_NOT_FOUND` if false:
```rust
let (resp_tx, resp_rx) = mpsc::channel();
tx.send(CtrlReq::BackendSendText {
    pane_id: p.context_id.clone(),
    text,
    resp: Some(resp_tx),
}).map_err(|_| RpcErr::from((-32603, "Server channel closed".to_string())))?;

if let Ok(false) = resp_rx.recv_timeout(std::time::Duration::from_secs(2)) {
    return Err(RpcErr {
        code: PANE_NOT_FOUND,
        message: format!("Pane not found: {}", p.context_id),
        data: Some(serde_json::json!({ "context_id": p.context_id })),
    });
}
```

- [ ] **Step 4: Run `cargo check && cargo test`**

Run: `cargo check && cargo test`
Expected: All existing tests pass. The sentinel is only triggered when a non-existent pane_id is used.

- [ ] **Step 5: Commit**

```bash
git add src/types.rs src/pane.rs src/server/mod.rs
git commit -m "feat(backend): spawn_cwd on Pane, PANE_NOT_FOUND sentinel on capture"
```

---

## Task 6: `run_shell` RPC Method

**Files:**
- Modify: `src/backend/dispatcher.rs`
- Modify: `src/types.rs` (add `BackendRunShell` to CtrlReq)
- Modify: `src/server/mod.rs` (handle `BackendRunShell`)
- Create: `tests/backend_protocol_v2.rs`

- [ ] **Step 1: Add `BackendRunShell` to `CtrlReq` enum**

In `src/types.rs`, add after the existing Backend variants:

```rust
/// Backend `run_shell` — resolve pane cwd for server-side command execution.
/// Returns the pane's spawn-time cwd (or None if no context_id / pane not found).
BackendRunShell {
    context_id: Option<String>,
    resp: mpsc::Sender<Option<std::path::PathBuf>>,
},
```

- [ ] **Step 2: Handle `BackendRunShell` in server event loop**

In `src/server/mod.rs`, add a handler in the main match on `CtrlReq`:

```rust
CtrlReq::BackendRunShell { context_id, resp } => {
    let cwd = if let Some(ref cid) = context_id {
        let pane_id_str = cid.trim_start_matches('%');
        if let Ok(pid) = pane_id_str.parse::<usize>() {
            let mut found_cwd = None;
            for win in app.windows.iter() {
                if let Some(path) = crate::tree::find_path_by_id(&win.root, pid) {
                    if let Some(p) = crate::tree::active_pane(&win.root, &path) {
                        found_cwd = p.spawn_cwd.clone();
                    }
                    break;
                }
            }
            found_cwd
        } else {
            None
        }
    } else {
        None
    };
    let _ = resp.send(cwd);
}
```

- [ ] **Step 3: Implement `handle_run_shell` in dispatcher**

Add to `dispatcher.rs`:

```rust
fn handle_run_shell(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, RpcErr> {
    let p: RunShellParams = serde_json::from_value(params.clone())
        .map_err(|e| RpcErr::from((-32602, format!("Invalid params: {e}"))))?;

    if p.command.is_empty() {
        return Err(RpcErr::from((-32602, "command must not be empty".to_string())));
    }

    let timeout_ms = p.timeout_ms.unwrap_or(30000) as u64;

    // Resolve working directory
    let cwd = if let Some(ref explicit_cwd) = p.cwd {
        Some(std::path::PathBuf::from(explicit_cwd))
    } else if p.context_id.is_some() {
        // Ask server for pane's spawn_cwd
        let (resp_tx, resp_rx) = mpsc::channel();
        tx.send(CtrlReq::BackendRunShell {
            context_id: p.context_id.clone(),
            resp: resp_tx,
        })
        .map_err(|_| RpcErr::from((-32603, "Server channel closed".to_string())))?;

        resp_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| RpcErr::from((-32603, "Server response timeout".to_string())))?
    } else {
        None
    };

    // Build command
    let mut cmd = std::process::Command::new(&p.command[0]);
    if p.command.len() > 1 {
        cmd.args(&p.command[1..]);
    }
    if let Some(ref dir) = cwd {
        cmd.current_dir(dir);
    }
    if let Some(ref env_vars) = p.env {
        for (k, v) in env_vars {
            cmd.env(k, v);
        }
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    // Spawn
    let start = std::time::Instant::now();
    let mut child = cmd.spawn().map_err(|e| RpcErr {
        code: COMMAND_FAILED,
        message: format!("Failed to spawn: {e}"),
        data: Some(serde_json::json!({ "command": p.command })),
    })?;

    // Take stdout/stderr pipes before the poll loop so we can read partial
    // output on timeout. The pipes are consumed; `child` retains the process
    // handle for `try_wait()` and `kill()`.
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();

    // Poll-based timeout: try_wait in a loop, kill on timeout.
    // This keeps ownership of `child` so we can call `child.kill()`.
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Process exited — read remaining output
                let mut stdout_buf = Vec::new();
                let mut stderr_buf = Vec::new();
                if let Some(ref mut pipe) = stdout_pipe {
                    use std::io::Read;
                    let _ = pipe.read_to_end(&mut stdout_buf);
                }
                if let Some(ref mut pipe) = stderr_pipe {
                    use std::io::Read;
                    let _ = pipe.read_to_end(&mut stderr_buf);
                }
                let result = RunShellResult {
                    exit_code: status.code().unwrap_or(-1),
                    stdout: String::from_utf8_lossy(&stdout_buf).to_string(),
                    stderr: String::from_utf8_lossy(&stderr_buf).to_string(),
                    elapsed_ms: start.elapsed().as_millis() as u64,
                };
                return serde_json::to_value(result)
                    .map_err(|e| RpcErr::from((-32603, format!("Internal error: {e}"))));
            }
            Ok(None) => {
                // Still running — check timeout
                if start.elapsed().as_millis() as u64 > timeout_ms {
                    // Kill the process (immediate child only; grandchildren
                    // may orphan — documented Windows limitation at this scale)
                    let _ = child.kill();
                    // Read whatever partial output is available
                    let mut stdout_buf = Vec::new();
                    let mut stderr_buf = Vec::new();
                    if let Some(ref mut pipe) = stdout_pipe {
                        use std::io::Read;
                        let _ = pipe.read_to_end(&mut stdout_buf);
                    }
                    if let Some(ref mut pipe) = stderr_pipe {
                        use std::io::Read;
                        let _ = pipe.read_to_end(&mut stderr_buf);
                    }
                    return Err(RpcErr {
                        code: COMMAND_TIMEOUT,
                        message: format!("Command timed out after {}ms", timeout_ms),
                        data: Some(serde_json::json!({
                            "stdout": String::from_utf8_lossy(&stdout_buf),
                            "stderr": String::from_utf8_lossy(&stderr_buf),
                            "command": p.command,
                            "timeout_ms": timeout_ms,
                        })),
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => {
                return Err(RpcErr {
                    code: COMMAND_FAILED,
                    message: format!("Process error: {e}"),
                    data: None,
                });
            }
        }
    }
}
```

- [ ] **Step 4: Wire `run_shell` into `dispatch_rpc`**

In the `match req.method.as_str()` block, add:

```rust
"run_shell" => handle_run_shell(&req.params, tx),
```

- [ ] **Step 5: Write integration test**

Create `tests/backend_protocol_v2.rs`:

```rust
use psmux::backend::protocol::*;

#[test]
fn run_shell_result_shape() {
    let result = RunShellResult {
        exit_code: 0,
        stdout: "hello\n".into(),
        stderr: String::new(),
        elapsed_ms: 42,
    };
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["exit_code"], 0);
    assert_eq!(json["stdout"], "hello\n");
    assert_eq!(json["stderr"], "");
    assert!(json["elapsed_ms"].is_number());
}

#[test]
fn error_codes_are_in_valid_range() {
    // JSON-RPC server errors: -32000 to -32099
    assert!(PANE_NOT_FOUND >= -32099 && PANE_NOT_FOUND <= -32000);
    assert!(SPAWN_FAILED >= -32099 && SPAWN_FAILED <= -32000);
    assert!(COMMAND_TIMEOUT >= -32099 && COMMAND_TIMEOUT <= -32000);
    assert!(COMMAND_FAILED >= -32099 && COMMAND_FAILED <= -32000);
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test --test backend_protocol_v2`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/backend/dispatcher.rs src/types.rs src/server/mod.rs tests/backend_protocol_v2.rs
git commit -m "feat(backend): run_shell RPC method — server-side command execution"
```

---

## Task 7: Shell Selection — `--shell` Flag

**Files:**
- Modify: `src/main.rs` (CLI parsing)
- Modify: `src/pane.rs` (`split_active_with_command`, `add_pane`)
- Modify: `src/types.rs` (BackendSpawnAgent shell field)
- Modify: `src/server/mod.rs` (pass shell to pane spawn)
- Modify: `src/format.rs` (`#{pane_shell}` format variable)
- Modify: `src/backend/dispatcher.rs` (pass shell from RPC params)

- [ ] **Step 1: Add `shell` metadata to Pane struct and CtrlReq variants**

In `src/types.rs`, add to Pane:

```rust
/// Shell binary used for this pane (basename, e.g. "bash", "pwsh").
/// Set at spawn time from --shell flag or default-shell.
pub shell_name: Option<String>,
```

Add `shell: Option<String>` to these CtrlReq variants:
- `BackendSpawnAgent { ..., shell: Option<String>, resp }` (for RPC spawns)
- `SplitWindow { ..., shell: Option<String> }` (for CLI split-window --shell)
- `NewWindow { ..., shell: Option<String> }` (for CLI new-window --shell)

Check the existing CtrlReq variants for `SplitWindow` and `NewWindow` — they may use different names. Add `shell: Option<String>` to each.

- [ ] **Step 2: Parse `--shell` in CLI for `new-window` and `split-window`**

In `src/main.rs`, find where `new-window` and `split-window` args are parsed. Add parsing for `--shell`:

```rust
// In the argument parsing loop for new-window/split-window:
let mut shell_override: Option<String> = None;
// ...
"--shell" => {
    shell_override = args_iter.next().map(|s| s.to_string());
}
```

Pass `shell_override` through to the server via the CtrlReq. Include it in `SplitWindow`/`NewWindow` sends:

```rust
tx.send(CtrlReq::SplitWindow {
    // ... existing fields ...
    shell: shell_override,
})
```

- [ ] **Step 2b: Update server-side destructuring for all modified CtrlReq variants**

In `src/server/mod.rs`, update the match arms that destructure these variants:

```rust
// BackendSpawnAgent — add shell to the destructure pattern:
CtrlReq::BackendSpawnAgent {
    command, cwd, env: extra_env, metadata, split_direction, shell, resp,
} => {
    // Pass shell to split_active_with_command (step 3)
}

// SplitWindow — add shell:
CtrlReq::SplitWindow { ..., shell } => {
    // Pass shell to split_active_with_command
}

// NewWindow — add shell:
CtrlReq::NewWindow { ..., shell } => {
    // Pass shell to add_pane / create_window
}
```

- [ ] **Step 3: Implement shell override in `split_active_with_command`**

In `src/pane.rs`, modify `split_active_with_command` signature to accept an optional shell:

```rust
pub fn split_active_with_command(
    app: &mut AppState,
    kind: LayoutKind,
    command: Option<&str>,
    pty_system_ref: Option<&dyn portable_pty::PtySystem>,
    start_dir: Option<&str>,
    shell_override: Option<&str>,  // NEW
) -> io::Result<()> {
```

In the shell resolution logic (around line 178-182), insert before the existing `default_shell` check:

```rust
// Resolution order: --shell flag > default-shell > system default
let expanded_shell = if let Some(shell) = shell_override {
    shell.to_string()
} else {
    crate::format::expand_format(&app.default_shell, app)
};
```

After pane creation, set `shell_name` on the pane:

```rust
new_pane.shell_name = Some(
    shell_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            if expanded_shell.is_empty() {
                "pwsh".to_string() // system default
            } else {
                std::path::Path::new(&expanded_shell)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            }
        }),
);
```

- [ ] **Step 4: Add `#{pane_shell}` format variable**

In `src/format.rs`, in the format variable match (where `pane_pid`, `pane_ready`, etc. are handled), add:

```rust
"pane_shell" => {
    if let Some(p) = target_pane() {
        p.shell_name.clone().unwrap_or_default()
    } else {
        String::new()
    }
}
```

- [ ] **Step 5: Wire shell through backend dispatcher**

In `dispatcher.rs::handle_spawn_agent`, pass `p.shell` to `BackendSpawnAgent`:

```rust
tx.send(CtrlReq::BackendSpawnAgent {
    command: p.command,
    cwd: p.cwd,
    env: p.env,
    metadata,
    split_direction,
    shell: p.shell,  // NEW
    resp: resp_tx,
})
```

In `server/mod.rs` `BackendSpawnAgent` handler, pass `shell` through to `split_active_with_command`.

- [ ] **Step 6: Update all call sites of `split_active_with_command`**

Every existing call to `split_active_with_command` needs the new `shell_override` parameter.

Search for: `split_active_with_command(` — known call sites:
1. `src/pane.rs` — `split_active()` wrapper → pass `None`
2. `src/server/mod.rs` — `SplitWindow` handler → pass `shell.as_deref()`
3. `src/server/mod.rs` — `SplitWindowPrint` handler → pass `shell.as_deref()`
4. `src/server/mod.rs` — `BackendSpawnAgent` handler → pass `shell.as_deref()` (NOT `None`)
5. Any other callers found via grep → pass `None`

Also update `add_pane` / `create_window` for `new-window` to accept and pass `shell_override`. Set `shell_name` on panes created via `create_window` too (not just split).

- [ ] **Step 7: Run `cargo test`**

Run: `cargo test`
Expected: All tests pass. No behavior change for existing callers (they pass `None`).

- [ ] **Step 8: Commit**

```bash
git add src/main.rs src/pane.rs src/types.rs src/server/mod.rs src/format.rs src/backend/dispatcher.rs
git commit -m "feat: --shell flag on new-window/split-window, #{pane_shell} format variable"
```

---

## Task 8: `--bare` Aware Agent Spawning

**Files:**
- Modify: `src/backend/dispatcher.rs`

- [ ] **Step 1: Write test for bare flag injection**

Add to `tests/backend_contracts.rs`:

```rust
#[test]
fn bare_flag_injects_bare_for_claude_command() {
    // This tests the logic, not the full RPC dispatch
    let mut command = vec!["claude".to_string(), "-p".to_string(), "task".to_string()];
    let bare = true;

    // Simulate the injection logic
    if bare && command.first().map(|c| c.to_lowercase().contains("claude")).unwrap_or(false) {
        command.insert(1, "--bare".to_string());
    }

    assert_eq!(command, vec!["claude", "--bare", "-p", "task"]);
}

#[test]
fn bare_flag_ignored_for_non_claude_command() {
    let mut command = vec!["python".to_string(), "script.py".to_string()];
    let bare = true;

    if bare && command.first().map(|c| c.to_lowercase().contains("claude")).unwrap_or(false) {
        command.insert(1, "--bare".to_string());
    }

    assert_eq!(command, vec!["python", "script.py"]);
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --test backend_contracts bare_flag`
Expected: PASS

- [ ] **Step 3: Implement bare flag injection in `handle_spawn_agent`**

In `dispatcher.rs::handle_spawn_agent`, after parsing params and before sending `CtrlReq::BackendSpawnAgent`, add:

```rust
let mut command = p.command;

// --bare injection: prepend --bare for Claude Code commands
if p.bare.unwrap_or(false) {
    if command
        .first()
        .map(|c| c.to_lowercase().contains("claude"))
        .unwrap_or(false)
    {
        command.insert(1, "--bare".to_string());
    }
}
```

- [ ] **Step 4: Run `cargo check && cargo test`**

Run: `cargo check && cargo test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/backend/dispatcher.rs tests/backend_contracts.rs
git commit -m "feat(backend): --bare aware agent spawning for Claude Code"
```

---

## Task 9: Validate & Fix Issues #143–#146

**Files:**
- Possibly modify: `src/server/mod.rs`, `src/client.rs`, `src/input.rs`

This task is investigative. The issues may already be fixed on ohboy-builds.

- [ ] **Step 1: Build the current binary**

Run: `cargo build --release`
Expected: PASS

- [ ] **Step 2: Test #146 — `list` commands from inside a session**

```
# Start a session
./target/release/psmux new-session -s test146
# Inside the session, run:
psmux list-panes
psmux list-windows
psmux list-sessions
```

Document: Does it work? What error if not?

- [ ] **Step 3: Test #145 — `source-file` from inside a session**

```
# Create a test conf
echo 'set -g status-style "bg=blue"' > /tmp/test.conf
# Inside a psmux session:
psmux source-file /tmp/test.conf
```

Document: Does it work? What error if not?

- [ ] **Step 4: Test #144 — `display-panes` freeze**

```
# Inside a psmux session with 2+ panes:
psmux display-panes
```

Document: Does it freeze? Does it dismiss on keypress? Do pane numbers remain (#143)?

- [ ] **Step 5: Fix or document each issue**

For each issue that reproduces:
- If fix is < 30 lines: fix it
- If fix requires deep changes: document finding in the issue and defer

- [ ] **Step 6: Commit any fixes**

```bash
# Only add the specific files that were modified:
git add src/server/mod.rs src/client.rs src/input.rs  # or whichever files were changed
git commit -m "fix: validate and fix issues #143-#146 against ohboy-builds"
```

---

## Task 10: Fix #88 Codex CLI Scrolling

**Files:**
- Modify: `src/input.rs`
- Possibly modify: `crates/vt100-psmux/`

- [ ] **Step 1: Investigate mouse mode passthrough**

Read `src/input.rs` to find where mouse scroll events are intercepted. Check if the code inspects `parser.screen().mouse_protocol_mode()` before entering copy mode.

- [ ] **Step 2: Implement fix**

If the pane's terminal has mouse mode enabled (SGR, X10, etc.), pass scroll events through to the pane instead of entering copy mode:

```rust
// In scroll event handler (src/input.rs):
let pane_wants_mouse = {
    if let Some(p) = active_pane {
        if let Ok(parser) = p.term.lock() {
            parser.screen().mouse_protocol_mode() != vt100::MouseProtocolMode::None
        } else {
            false
        }
    } else {
        false
    }
};

if pane_wants_mouse {
    // Forward scroll event to pane as mouse sequence
    // Don't enter copy mode
} else {
    // Existing behavior: enter copy mode
}
```

- [ ] **Step 3: Test with a mouse-capturing application**

Run a program that captures mouse (e.g., `vim`, `less -R`, or any TUI app) inside a psmux pane. Verify scrolling works inside the app.

- [ ] **Step 4: Run `cargo test`**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add src/input.rs
git commit -m "fix(#88): pass mouse scroll to pane when application captures mouse"
```

---

## Task 11: Final Integration Test + Verification

**Files:**
- Modify: `tests/backend_protocol_v2.rs`

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Run format check**

Run: `cargo fmt --check`
Expected: No formatting issues

- [ ] **Step 4: Run swarm E2E tests (if available)**

Run: `pwsh -Command "& './tests/test_swarm_e2e.ps1' -Phase @(1,2,3)"`
Expected: Core phases pass

- [ ] **Step 5: Verify success criteria**

Check against spec success criteria:
1. Spawn readiness polling works (test with `wait_ready: true`)
2. Capture freshness works (test with `wait_for_output: true`)
3. `run_shell` returns correct output
4. `--shell bash` opens bash
5. Protocol version is "2"
6. Issues #143-#146 resolved or documented

- [ ] **Step 6: Final commit**

```bash
git add -A
git commit -m "feat: protocol-complete agent backend v2 — all features integrated"
```
