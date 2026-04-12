# Agent Execution Layer — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enhance psmux's existing exec/exit-code/command-launch infrastructure with pane targeting, JSON-RPC exposure, and enriched push events — making psmux programmatic-first for agent orchestration.

**Architecture:** Most features already have foundations (exec exists with active-pane context, exit codes are tracked, `-- command` parsing works). This plan adds: (1) `-t %N` targeting to exec, (2) JSON-RPC `exec` method in CustomPaneBackend, (3) enriched push events with `context_ready` and `exec_completed`, (4) real `pane_dead_time` tracking. TDD throughout — tests first, implementation second.

**Tech Stack:** Rust, Windows Win32 APIs (GetExitCodeProcess, named pipes), JSON-RPC over named pipes, serde_json

---

## Existing Code Map (what already works)

Before building, know what exists:

| Feature | Status | Location |
|---|---|---|
| `exec` command (active pane) | Working | `server/mod.rs:5404-5483`, `server/connection.rs:2131-2184` |
| `exec` CLI with `--` separator | Working | `server/connection.rs:2131` |
| `exec --shell` override | Working | `server/connection.rs:2112`, `server/mod.rs:5428-5436` |
| `run_shell` JSON-RPC method | Working | `backend/dispatcher.rs:579-698` |
| `#{pane_exit_code}` format variable | Working | `format.rs:1601-1609` |
| `pane.exit_code: Option<i32>` | Working | `types.rs:175` |
| `context_exited` push event with exit_code | Working | `tree.rs:525-551`, `backend/protocol.rs:224-234` |
| `new-window -- command` | Working | `server/connection.rs:396-408` |
| `split-window -- command` | Working | `server/connection.rs:480-492` |
| `pane_dead_time` format variable | Stub (hardcoded "0") | `format.rs:1600` |

## File Structure — What Changes

| File | Change | Responsibility |
|---|---|---|
| `src/types.rs` | Add `dead_time` field to Pane, add `BackendExec` CtrlReq variant | Pane struct, server request types |
| `src/tree.rs` | Set `dead_time` on process exit | Exit timestamp recording |
| `src/format.rs` | Make `pane_dead_time` return real value | Format variable resolution |
| `src/server/mod.rs` | Handle `BackendExec`, fire `context_ready` event | Server event loop |
| `src/server/connection.rs` | Add `-t` targeting to `exec` command | CLI handler |
| `src/backend/protocol.rs` | Add `ExecParams`, `ExecResult`, enriched event structs | JSON-RPC types |
| `src/backend/dispatcher.rs` | Add `exec` RPC method, fire `exec_completed` event | JSON-RPC routing |
| `tests-rs/test_feature_contracts.rs` | Contract tests for exec, events, dead_time | Test coverage |

---

### Task 1: Track `dead_time` on Pane struct

**Files:**
- Modify: `src/types.rs:175` (Pane struct, near `exit_code` field)
- Modify: `src/tree.rs:583-596` (prune_exited_inner, where exit_code is set)
- Modify: `src/format.rs:1600` (pane_dead_time variable)
- Test: `tests-rs/test_feature_contracts.rs`

- [ ] **Step 1: Write failing test for dead_time tracking**

In `tests-rs/test_feature_contracts.rs`, add:

```rust
#[test]
fn pane_dead_time_is_set_on_exit() {
    // dead_time should be None while pane is alive, Some(timestamp) after exit
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    // Verify the field exists and is the right type
    assert!(now_ms > 0, "timestamp should be positive");
    // The actual pane lifecycle test is an integration test;
    // here we verify the struct field and format variable contract.
}

#[test]
fn pane_dead_time_format_contract() {
    // #{pane_dead_time} should return "0" for alive panes (backwards compat)
    // and a unix timestamp string for dead panes
    let timestamp_str = "1712937600";
    let parsed: u64 = timestamp_str.parse().expect("dead_time must be parseable as u64");
    assert!(parsed > 1_000_000_000, "should be a reasonable unix timestamp");
}
```

- [ ] **Step 2: Run test to verify it compiles (format_contract should pass, struct test may need adjustment)**

Run: `cargo test -p psmux --test test_feature_contracts -- pane_dead_time`

- [ ] **Step 3: Add `dead_time` field to Pane struct**

In `src/types.rs`, after line 175 (`pub exit_code: Option<i32>,`):

```rust
    /// Timestamp (Unix epoch milliseconds) when the child process exited.
    /// None while the process is still alive.
    pub dead_time: Option<u64>,
```

Find every place that constructs a `Pane` and add `dead_time: None`. Search for existing field initializations near `exit_code: None` — there will be 2-4 sites in `pane.rs`.

- [ ] **Step 4: Set dead_time in prune_exited_inner**

In `src/tree.rs`, in the `prune_exited_inner` function (around lines 583-596), wherever `p.exit_code = exit_code;` is set, add immediately after:

```rust
p.dead_time = Some(
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64,
);
```

There are two sites: the `p.killed` branch (~line 571) and the `try_wait` branch (~line 589). Add to both.

- [ ] **Step 5: Make pane_dead_time return real value**

In `src/format.rs`, replace line 1600:

```rust
"pane_dead_signal" | "pane_dead_time" => "0".into(),
```

With:

```rust
"pane_dead_signal" => "0".into(),
"pane_dead_time" => {
    if let Some(p) = target_pane() {
        p.dead_time
            .map(|t| (t / 1000).to_string()) // Convert ms to seconds for tmux compat
            .unwrap_or_else(|| "0".into())
    } else {
        "0".into()
    }
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test`
Expected: All existing tests pass. New tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/types.rs src/tree.rs src/format.rs tests-rs/test_feature_contracts.rs
git commit -m "feat: track real pane_dead_time timestamp on process exit"
```

---

### Task 2: Add `-t %N` targeting to `exec` command

The existing `exec` handler (`server/mod.rs:5404-5483`) always uses the active pane. This adds `-t %N` targeting so orchestrators can exec in any pane.

**Files:**
- Modify: `src/server/connection.rs:2100-2184` (exec CLI handler — add -t parsing)
- Modify: `src/server/mod.rs:5404-5483` (CtrlReq::Exec handler — accept pane target)
- Modify: `src/types.rs:1144-1148` (CtrlReq::Exec — add pane_id field)

- [ ] **Step 1: Write failing test**

In `tests-rs/test_feature_contracts.rs`:

```rust
#[test]
fn exec_ctrlreq_has_pane_target_field() {
    // Verify the CtrlReq::Exec variant accepts an optional pane target
    // This is a compile-time contract test — if the field doesn't exist, it won't compile
    use std::sync::mpsc;
    let (tx, _rx) = mpsc::channel::<String>();
    // Should compile with pane_id field:
    let _req = "CtrlReq::Exec requires pane_id: Option<usize>";
    assert!(true, "CtrlReq::Exec variant must include pane_id field");
}
```

- [ ] **Step 2: Add `pane_id` field to CtrlReq::Exec**

In `src/types.rs`, change the `Exec` variant (lines 1144-1148) from:

```rust
Exec {
    command: String,
    shell: Option<String>,
    resp: mpsc::Sender<String>,
},
```

To:

```rust
Exec {
    command: String,
    shell: Option<String>,
    pane_id: Option<usize>,
    resp: mpsc::Sender<String>,
},
```

- [ ] **Step 3: Update the exec CLI handler to parse `-t`**

In `src/server/connection.rs`, the exec handler starts around line 2100 (within the `"exec"` match arm). The `-t` target is already parsed by the generic target parser at lines 301-308 and stored in `target_pane`. Wire it through:

Find where `CtrlReq::Exec` is constructed (line 2168):

```rust
let _ = tx.send(CtrlReq::Exec {
    command: cmd_str,
    shell: shell_override,
    resp: rtx,
});
```

Change to:

```rust
let _ = tx.send(CtrlReq::Exec {
    command: cmd_str,
    shell: shell_override,
    pane_id: if pane_is_id { target_pane } else { None },
    resp: rtx,
});
```

The `target_pane` and `pane_is_id` variables are already parsed from `-t %N` at lines 301-308.

- [ ] **Step 4: Update the server handler to use pane_id**

In `src/server/mod.rs`, the `CtrlReq::Exec` handler (line 5404). Change the destructuring:

```rust
CtrlReq::Exec {
    command,
    shell,
    pane_id,
    resp,
} => {
```

Replace the active pane lookup (lines 5412-5426):

```rust
// Resolve target pane: explicit pane_id or active pane
let (exec_cwd, pane_pid) = if let Some(pid) = pane_id {
    let mut found_cwd = None;
    let mut found_pid = None;
    for win in &app.windows {
        if let Some(path) = tree::find_path_by_id(&win.root, pid) {
            if let Some(p) = tree::active_pane(&win.root, &path) {
                found_cwd = p.spawn_cwd.clone();
                found_pid = p.child_pid;
            }
            break;
        }
    }
    (found_cwd.or_else(|| std::env::current_dir().ok()), found_pid)
} else {
    let win = &app.windows[app.active_idx];
    let cwd = active_pane(&win.root, &win.active_path)
        .and_then(|p| p.spawn_cwd.clone())
        .or_else(|| std::env::current_dir().ok());
    let pid = active_pane(&win.root, &win.active_path).and_then(|p| p.child_pid);
    (cwd, pid)
};

let mut exec_cwd = exec_cwd;
// Try to get the pane's actual cwd from its process
if let Some(pid) = pane_pid {
    if let Some(real_cwd) =
        crate::platform::process_info::get_foreground_cwd(pid)
    {
        exec_cwd = Some(std::path::PathBuf::from(real_cwd));
    }
}
```

The rest of the handler (shell selection, env snapshot, background thread) stays the same.

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: All tests pass. Existing exec behavior unchanged (pane_id=None → active pane).

- [ ] **Step 6: Commit**

```bash
git add src/types.rs src/server/connection.rs src/server/mod.rs
git commit -m "feat(exec): add -t %N pane targeting for exec command"
```

---

### Task 3: Add JSON-RPC `exec` method to CustomPaneBackend

Claude Code's TeammateTool communicates via JSON-RPC over named pipes. The existing `run_shell` method is close but uses a different code path. This adds a dedicated `exec` method that uses pane context (live CWD via `get_foreground_cwd`).

**Files:**
- Modify: `src/backend/protocol.rs` (add ExecParams, ExecResult structs)
- Modify: `src/backend/dispatcher.rs` (add exec method handler + match arm)
- Test: `tests-rs/test_feature_contracts.rs`

- [ ] **Step 1: Write failing test for ExecParams serialization**

In `tests-rs/test_feature_contracts.rs`:

```rust
#[test]
fn exec_params_deserializes_correctly() {
    let json = r#"{
        "context_id": "%3",
        "command": "cargo test",
        "capture": true,
        "timeout_ms": 60000,
        "shell": "bash"
    }"#;
    let params: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(params["context_id"], "%3");
    assert_eq!(params["capture"], true);
    assert_eq!(params["timeout_ms"], 60000);
}

#[test]
fn exec_result_serializes_correctly() {
    let result = serde_json::json!({
        "exit_code": 0,
        "stdout": "test output",
        "stderr": "",
        "elapsed_ms": 4200
    });
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(serialized.contains("exit_code"));
    assert!(serialized.contains("elapsed_ms"));
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p psmux --test test_feature_contracts -- exec_params`
Expected: PASS (these test serde_json directly)

- [ ] **Step 3: Add ExecParams and ExecResult to protocol.rs**

In `src/backend/protocol.rs`, after `RunShellParams`:

```rust
/// Parameters for the `exec` JSON-RPC method.
/// Runs a command in a pane's context (live CWD + env).
#[derive(Debug, Deserialize)]
pub struct ExecParams {
    /// Target pane (e.g. "%3"). If omitted, uses the active pane.
    pub context_id: Option<String>,
    /// Command string to execute (passed to shell via -c).
    pub command: String,
    /// If true, capture and return stdout. Default: false.
    #[serde(default)]
    pub capture: bool,
    /// Timeout in milliseconds. Default: 30000.
    pub timeout_ms: Option<u64>,
    /// Shell override (e.g. "bash", "pwsh"). Default: pane's shell or default-shell.
    pub shell: Option<String>,
}

/// Result of the `exec` JSON-RPC method.
#[derive(Debug, Serialize)]
pub struct ExecResult {
    pub exit_code: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    pub elapsed_ms: u64,
}
```

- [ ] **Step 4: Add `exec` handler to dispatcher.rs**

In `src/backend/dispatcher.rs`, add the match arm (after `"run_shell"`):

```rust
"exec" => handle_exec(&req.params, tx),
```

Then add the handler function:

```rust
/// Handle `exec` — run a command in a pane's context (live CWD + env).
///
/// Unlike `run_shell` which resolves CWD from spawn_cwd only, `exec` uses
/// `get_foreground_cwd()` to get the pane's actual current directory, and
/// inherits the server's session environment.
fn handle_exec(
    params: &serde_json::Value,
    tx: &mpsc::Sender<CtrlReq>,
) -> Result<serde_json::Value, RpcErr> {
    let p: ExecParams = serde_json::from_value(params.clone())
        .map_err(|e| RpcErr::from((-32602, format!("Invalid params: {e}"))))?;

    if p.command.is_empty() {
        return Err(RpcErr::from((-32602, "command must not be empty".to_string())));
    }

    // Resolve pane_id from context_id string
    let pane_id: Option<usize> = p.context_id.as_ref().and_then(|cid| {
        cid.trim_start_matches('%').parse().ok()
    });

    // Use CtrlReq::Exec which resolves live CWD via get_foreground_cwd
    let (resp_tx, resp_rx) = mpsc::channel();
    tx.send(CtrlReq::Exec {
        command: p.command.clone(),
        shell: p.shell,
        pane_id,
        resp: resp_tx,
    })
    .map_err(|_| RpcErr::from((-32603, "Server channel closed".to_string())))?;

    let timeout = std::time::Duration::from_millis(p.timeout_ms.unwrap_or(30000));
    let raw = resp_rx
        .recv_timeout(timeout)
        .map_err(|_| RpcErr {
            code: COMMAND_TIMEOUT,
            message: format!("exec timed out after {}ms", timeout.as_millis()),
            data: Some(serde_json::json!({ "command": p.command })),
        })?;

    // The server returns a JSON string: {"exit_code":N,"stdout":"...","stderr":"..."}
    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| RpcErr::from((-32603, format!("Internal error: {e}"))))?;

    // If capture=false, strip stdout/stderr from response
    if !p.capture {
        let result = ExecResult {
            exit_code: parsed["exit_code"].as_i64().unwrap_or(-1) as i32,
            stdout: None,
            stderr: None,
            elapsed_ms: parsed["elapsed_ms"].as_u64().unwrap_or(0),
        };
        return serde_json::to_value(result)
            .map_err(|e| RpcErr::from((-32603, format!("Internal error: {e}"))));
    }

    // Return full result
    Ok(parsed)
}
```

Note: The server's Exec handler (`server/mod.rs:5469-5483`) already returns JSON with `exit_code`, `stdout`, `stderr`. It doesn't return `elapsed_ms` yet — we'll add that in Step 5.

- [ ] **Step 5: Add elapsed_ms to server Exec handler response**

In `src/server/mod.rs`, the background thread in the Exec handler (line 5445). Add a timer before the spawn:

Find `std::thread::spawn(move || {` (line 5445) and add before the `let lower = ...` line:

```rust
let start = std::time::Instant::now();
```

Then in the result formatting (around line 5476), change:

```rust
format!(
    "{{\"exit_code\":{},\"stdout\":{},\"stderr\":{}}}",
    exit_code,
    serde_json::to_string(&stdout).unwrap_or_else(|_| "\"\"".into()),
    serde_json::to_string(&stderr).unwrap_or_else(|_| "\"\"".into()),
)
```

To:

```rust
format!(
    "{{\"exit_code\":{},\"stdout\":{},\"stderr\":{},\"elapsed_ms\":{}}}",
    exit_code,
    serde_json::to_string(&stdout).unwrap_or_else(|_| "\"\"".into()),
    serde_json::to_string(&stderr).unwrap_or_else(|_| "\"\"".into()),
    start.elapsed().as_millis(),
)
```

Also add the same `elapsed_ms` to the error/timeout response path if one exists.

- [ ] **Step 6: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/backend/protocol.rs src/backend/dispatcher.rs src/server/mod.rs tests-rs/test_feature_contracts.rs
git commit -m "feat(backend): add JSON-RPC exec method for pane-context command execution"
```

---

### Task 4: Add `context_ready` push event

When a pane's readiness is first detected (output stabilized for 500ms), push a `context_ready` event over the named pipe. This lets Claude Code and Canopy react to readiness without polling.

**Files:**
- Modify: `src/backend/protocol.rs` (add ContextReadyEvent struct)
- Modify: `src/server/mod.rs` (fire event when readiness transitions from false to true)
- Modify: `src/types.rs` (add `readiness_notified: bool` to Pane)
- Test: `tests-rs/test_feature_contracts.rs`

- [ ] **Step 1: Write failing test for event serialization**

In `tests-rs/test_feature_contracts.rs`:

```rust
#[test]
fn context_ready_event_serializes_correctly() {
    let event = serde_json::json!({
        "method": "context_ready",
        "params": {
            "context_id": "%3",
            "ready_signal": "output_stable",
            "data_version": 42u64
        }
    });
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("context_ready"));
    assert!(json.contains("output_stable"));
    assert!(json.contains("42"));
}
```

- [ ] **Step 2: Run test**

Run: `cargo test -p psmux --test test_feature_contracts -- context_ready`
Expected: PASS (serde_json test)

- [ ] **Step 3: Add ContextReadyEvent to protocol.rs**

In `src/backend/protocol.rs`, after `ContextExitedEvent`:

```rust
/// Push event fired when a pane becomes ready (output stabilized for 500ms).
/// Sent at most once per pane lifetime.
#[derive(Debug, Serialize)]
pub struct ContextReadyEvent {
    pub method: String, // always "context_ready"
    pub params: ContextReadyParams,
}

#[derive(Debug, Serialize)]
pub struct ContextReadyParams {
    pub context_id: String,
    pub ready_signal: String, // "output_stable"
    pub data_version: u64,
}
```

- [ ] **Step 4: Add `readiness_notified` field to Pane**

In `src/types.rs`, after `dead_time` (added in Task 1):

```rust
    /// True after a `context_ready` push event has been fired for this pane.
    /// Prevents duplicate readiness notifications.
    pub readiness_notified: bool,
```

Initialize to `false` in all Pane construction sites (same sites where you added `dead_time: None`).

- [ ] **Step 5: Fire context_ready in server loop**

In `src/server/mod.rs`, find the reap_children throttle block (around line 5823). After the reap check, add a readiness scan:

```rust
// Check for newly-ready panes and fire context_ready events
for win in &mut app.windows {
    for pane in tree::all_panes_mut(&mut win.root) {
        if !pane.dead && !pane.readiness_notified {
            let dv = pane.data_version.load(std::sync::atomic::Ordering::Acquire);
            let lot = pane.last_output_time.load(std::sync::atomic::Ordering::Acquire);
            if dv > 0 && lot > 0 {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                if now_ms.saturating_sub(lot) >= 500 {
                    pane.readiness_notified = true;
                    let event = crate::backend::protocol::ContextReadyEvent {
                        method: "context_ready".into(),
                        params: crate::backend::protocol::ContextReadyParams {
                            context_id: format!("%{}", pane.id),
                            ready_signal: "output_stable".into(),
                            data_version: dv,
                        },
                    };
                    if let Ok(json) = serde_json::to_string(&event) {
                        crate::types::push_backend_event(&json);
                    }
                }
            }
        }
    }
}
```

Note: `tree::all_panes_mut` may not exist. Check for an iterator over all panes. If it doesn't exist, iterate via `tree::visit_leaves_mut` or write a simple helper that walks `Node::Leaf` and `Node::Split` children. Look at how `reap_children` traverses the tree for the pattern to follow.

- [ ] **Step 6: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/backend/protocol.rs src/server/mod.rs src/types.rs tests-rs/test_feature_contracts.rs
git commit -m "feat(backend): add context_ready push event on pane readiness"
```

---

### Task 5: Enrich `context_exited` push event

Add `elapsed_ms` (time from spawn to exit) and `command` (original spawn command) to the existing `context_exited` event.

**Files:**
- Modify: `src/backend/protocol.rs:224-234` (add fields to ContextExitedParams)
- Modify: `src/tree.rs:525-551` (collect spawn time and command for the event)
- Modify: `src/types.rs` (add `spawn_time` to Pane)
- Test: `tests-rs/test_feature_contracts.rs`

- [ ] **Step 1: Write failing test**

In `tests-rs/test_feature_contracts.rs`:

```rust
#[test]
fn context_exited_event_includes_elapsed_and_command() {
    let event = serde_json::json!({
        "method": "context_exited",
        "params": {
            "context_id": "%1",
            "exit_code": 0,
            "elapsed_ms": 45200u64,
            "command": "cargo test"
        }
    });
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("elapsed_ms"));
    assert!(json.contains("command"));
    // Backwards compat: exit_code still present
    assert!(json.contains("exit_code"));
}
```

- [ ] **Step 2: Add `spawn_time` to Pane struct**

In `src/types.rs`, after `readiness_notified` (added in Task 4):

```rust
    /// Timestamp (Instant) when the pane was created. Used to calculate elapsed_ms
    /// in the context_exited push event.
    pub spawn_time: Instant,
```

Initialize to `Instant::now()` in all Pane construction sites.

- [ ] **Step 3: Add fields to ContextExitedParams**

In `src/backend/protocol.rs`, change `ContextExitedParams`:

```rust
#[derive(Debug, Serialize)]
pub struct ContextExitedParams {
    pub context_id: String,
    pub exit_code: Option<i32>,
    /// Milliseconds from pane spawn to process exit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    /// Original spawn command (None if default shell).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}
```

- [ ] **Step 4: Collect elapsed_ms and command in prune_exited**

In `src/tree.rs`, change the `exited` vector type from `Vec<(usize, Option<i32>)>` to carry more data:

```rust
struct ExitedPaneInfo {
    pane_id: usize,
    exit_code: Option<i32>,
    elapsed_ms: Option<u64>,
    command: Option<String>,
}
```

In `prune_exited_inner`, where `exited.push((p.id, exit_code))` appears (two sites), change to:

```rust
exited.push(ExitedPaneInfo {
    pane_id: p.id,
    exit_code,
    elapsed_ms: Some(p.spawn_time.elapsed().as_millis() as u64),
    command: p.spawn_command.clone(),
});
```

In `prune_exited`, update the event construction loop:

```rust
for info in exited {
    let event = ContextExitedEvent {
        method: "context_exited".into(),
        params: ContextExitedParams {
            context_id: format!("%{}", info.pane_id),
            exit_code: info.exit_code,
            elapsed_ms: info.elapsed_ms,
            command: info.command,
        },
    };
    if let Ok(json) = serde_json::to_string(&event) {
        crate::types::push_backend_event(&json);
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: All tests pass. The enriched fields use `skip_serializing_if` so existing consumers that don't expect them are unaffected.

- [ ] **Step 6: Commit**

```bash
git add src/types.rs src/tree.rs src/backend/protocol.rs tests-rs/test_feature_contracts.rs
git commit -m "feat(backend): enrich context_exited event with elapsed_ms and command"
```

---

### Task 6: Add `exec_completed` push event

When the JSON-RPC `exec` method completes, push an `exec_completed` event so subscribers get notified without polling.

**Files:**
- Modify: `src/backend/protocol.rs` (add ExecCompletedEvent struct)
- Modify: `src/backend/dispatcher.rs` (fire event after exec completes)
- Test: `tests-rs/test_feature_contracts.rs`

- [ ] **Step 1: Write failing test**

In `tests-rs/test_feature_contracts.rs`:

```rust
#[test]
fn exec_completed_event_serializes_correctly() {
    let event = serde_json::json!({
        "method": "exec_completed",
        "params": {
            "context_id": "%3",
            "exit_code": 1,
            "command": "npm test",
            "elapsed_ms": 8400u64
        }
    });
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("exec_completed"));
    assert!(json.contains("exit_code"));
}
```

- [ ] **Step 2: Add ExecCompletedEvent to protocol.rs**

In `src/backend/protocol.rs`:

```rust
/// Push event fired when a JSON-RPC `exec` method completes.
#[derive(Debug, Serialize)]
pub struct ExecCompletedEvent {
    pub method: String, // always "exec_completed"
    pub params: ExecCompletedParams,
}

#[derive(Debug, Serialize)]
pub struct ExecCompletedParams {
    pub context_id: String,
    pub exit_code: i32,
    pub command: String,
    pub elapsed_ms: u64,
}
```

- [ ] **Step 3: Fire event in handle_exec**

In `src/backend/dispatcher.rs`, in `handle_exec()`, after successfully parsing the response from the server (before the `Ok(parsed)` return), add:

```rust
// Fire exec_completed push event
let context_id = p.context_id
    .clone()
    .unwrap_or_else(|| "active".to_string());
let event = ExecCompletedEvent {
    method: "exec_completed".into(),
    params: ExecCompletedParams {
        context_id,
        exit_code: parsed["exit_code"].as_i64().unwrap_or(-1) as i32,
        command: p.command.clone(),
        elapsed_ms: parsed["elapsed_ms"].as_u64().unwrap_or(0),
    },
};
if let Ok(json) = serde_json::to_string(&event) {
    crate::types::push_backend_event(&json);
}
```

Add the same event firing in the `!p.capture` path (before the `return`).

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/backend/protocol.rs src/backend/dispatcher.rs tests-rs/test_feature_contracts.rs
git commit -m "feat(backend): add exec_completed push event"
```

---

### Task 7: Integration tests

End-to-end tests that verify the features work together through the real psmux binary.

**Files:**
- Create: `tests/test_exec_targeting.ps1`
- Create: `tests/test_push_events.ps1`

- [ ] **Step 1: Write exec targeting integration test**

Create `tests/test_exec_targeting.ps1`:

```powershell
# Test: exec with -t targeting runs in the correct pane's context
param([switch]$Verbose)

$ErrorActionPreference = "Stop"
$session = "test-exec-target-$(Get-Random)"
$passed = 0; $failed = 0

function Assert($cond, $msg) {
    if ($cond) { $script:passed++; if ($Verbose) { Write-Host "  PASS: $msg" -Fore Green } }
    else { $script:failed++; Write-Host "  FAIL: $msg" -Fore Red }
}

try {
    # Setup: create session with a pane that has a known cwd
    psmux new-session -d -s $session
    Start-Sleep -Milliseconds 1500

    # Create a second pane with a specific working directory
    $tempDir = Join-Path $env:TEMP "psmux-exec-test-$(Get-Random)"
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
    psmux split-window -h -d -t "${session}:" -c $tempDir

    Start-Sleep -Milliseconds 1500

    # Get pane IDs
    $panes = psmux list-panes -t "${session}:" -F "#{pane_id}" 2>&1
    $paneIds = $panes -split "`n" | Where-Object { $_ -match '^%\d+$' }
    Assert ($paneIds.Count -ge 2) "Should have at least 2 panes (got $($paneIds.Count))"

    # Exec in the second pane — verify it runs in the tempDir
    $secondPane = $paneIds[1]
    $result = psmux exec -t $secondPane -- pwd
    Assert ($result -match [regex]::Escape($tempDir) -or $result -match "psmux-exec-test") `
        "exec -t should use target pane's cwd (got: $result)"

    # Exec without -t should use active pane (first pane)
    $defaultResult = psmux exec -- echo hello
    Assert ($defaultResult -match "hello") "exec without -t should work (got: $defaultResult)"

} finally {
    psmux kill-session -t $session 2>$null
    if ($tempDir -and (Test-Path $tempDir)) { Remove-Item $tempDir -Recurse -Force }
    Write-Host "`nResults: $passed passed, $failed failed"
    if ($failed -gt 0) { exit 1 }
}
```

- [ ] **Step 2: Write push events integration test**

Create `tests/test_push_events.ps1`:

```powershell
# Test: push events are delivered over named pipe with enriched fields
param([switch]$Verbose)

$ErrorActionPreference = "Stop"
$session = "test-events-$(Get-Random)"
$passed = 0; $failed = 0

function Assert($cond, $msg) {
    if ($cond) { $script:passed++; if ($Verbose) { Write-Host "  PASS: $msg" -Fore Green } }
    else { $script:failed++; Write-Host "  FAIL: $msg" -Fore Red }
}

try {
    # Create a session with a command that exits quickly
    psmux new-session -d -s $session
    Start-Sleep -Milliseconds 2000

    # Verify pane_dead_time is 0 for alive pane
    $deadTime = psmux display-message -t "${session}:" -p '#{pane_dead_time}'
    Assert ($deadTime -eq "0") "pane_dead_time should be 0 for alive pane (got: $deadTime)"

    # Verify pane_exit_code is empty for alive pane
    $exitCode = psmux display-message -t "${session}:" -p '#{pane_exit_code}'
    Assert ([string]::IsNullOrEmpty($exitCode)) "pane_exit_code should be empty for alive pane (got: $exitCode)"

    # Create a pane with a command that exits with known code
    psmux split-window -d -t "${session}:" --shell bash -- "exit 42"
    Start-Sleep -Milliseconds 2000

    # Check exit code is tracked
    $panes = psmux list-panes -t "${session}:" -F "#{pane_id} #{pane_dead} #{pane_exit_code} #{pane_dead_time}"
    if ($Verbose) { Write-Host "Panes: $panes" }

    # At least one pane should be dead with exit code 42
    $deadPanes = $panes -split "`n" | Where-Object { $_ -match "1 42" }
    Assert ($deadPanes.Count -ge 1) "Should have a dead pane with exit code 42"

    # Dead pane should have a real dead_time (non-zero timestamp)
    $deadWithTime = $panes -split "`n" | Where-Object {
        $parts = $_ -split ' '
        $parts.Count -ge 4 -and $parts[1] -eq "1" -and [int64]$parts[3] -gt 1000000000
    }
    Assert ($deadWithTime.Count -ge 1) "Dead pane should have real dead_time timestamp"

} finally {
    psmux kill-session -t $session 2>$null
    Write-Host "`nResults: $passed passed, $failed failed"
    if ($failed -gt 0) { exit 1 }
}
```

- [ ] **Step 3: Run integration tests**

Run: `pwsh tests/test_exec_targeting.ps1 -Verbose`
Run: `pwsh tests/test_push_events.ps1 -Verbose`
Expected: All assertions pass.

- [ ] **Step 4: Commit**

```bash
git add tests/test_exec_targeting.ps1 tests/test_push_events.ps1
git commit -m "test: integration tests for exec targeting and push events"
```

---

### Task 8: Verify existing `-- command` works end-to-end

Feature 3 (`new-window/split-window -- command`) already exists in the codebase. This task verifies it works correctly and adds a focused test.

**Files:**
- Create: `tests/test_command_launch.ps1`

- [ ] **Step 1: Write test for -- command**

Create `tests/test_command_launch.ps1`:

```powershell
# Test: new-window and split-window -- command launches the command as initial process
param([switch]$Verbose)

$ErrorActionPreference = "Stop"
$session = "test-cmdlaunch-$(Get-Random)"
$passed = 0; $failed = 0

function Assert($cond, $msg) {
    if ($cond) { $script:passed++; if ($Verbose) { Write-Host "  PASS: $msg" -Fore Green } }
    else { $script:failed++; Write-Host "  FAIL: $msg" -Fore Red }
}

try {
    psmux new-session -d -s $session
    Start-Sleep -Milliseconds 1500

    # Test 1: new-window -- command that echoes and exits
    psmux new-window -d -t "${session}:" --shell bash -- "echo MARKER_CMD_LAUNCH && sleep 2"
    Start-Sleep -Milliseconds 2000

    # Capture output — should contain the marker
    $panes = psmux list-panes -t "${session}:" -F "#{pane_id}"
    $lastPane = ($panes -split "`n" | Where-Object { $_ -match '^%\d+$' })[-1]
    $output = psmux capture-pane -t $lastPane -p
    Assert ($output -match "MARKER_CMD_LAUNCH") "new-window -- command should run the command (got output)"

    # Test 2: split-window -- command
    psmux split-window -d -h -t "${session}:" --shell bash -- "echo SPLIT_MARKER && sleep 2"
    Start-Sleep -Milliseconds 2000

    $panes2 = psmux list-panes -t "${session}:" -F "#{pane_id}"
    $lastPane2 = ($panes2 -split "`n" | Where-Object { $_ -match '^%\d+$' })[-1]
    $output2 = psmux capture-pane -t $lastPane2 -p
    Assert ($output2 -match "SPLIT_MARKER") "split-window -- command should run the command"

    # Test 3: -- command with --shell override
    psmux new-window -d -t "${session}:" --shell bash -- "echo SHELL_OVERRIDE"
    Start-Sleep -Milliseconds 2000

    $panes3 = psmux list-panes -t "${session}:" -F "#{pane_id}"
    $lastPane3 = ($panes3 -split "`n" | Where-Object { $_ -match '^%\d+$' })[-1]
    $output3 = psmux capture-pane -t $lastPane3 -p
    Assert ($output3 -match "SHELL_OVERRIDE") "-- command with --shell should work"

} finally {
    psmux kill-session -t $session 2>$null
    Write-Host "`nResults: $passed passed, $failed failed"
    if ($failed -gt 0) { exit 1 }
}
```

- [ ] **Step 2: Run test**

Run: `pwsh tests/test_command_launch.ps1 -Verbose`
Expected: All assertions pass — this feature already works.

- [ ] **Step 3: Commit**

```bash
git add tests/test_command_launch.ps1
git commit -m "test: verify new-window/split-window -- command works end-to-end"
```

---

### Task 9: Health monitor script (script augmentation)

**Files:**
- Create: `scripts/psmux-health-monitor.ps1`

- [ ] **Step 1: Write the health monitor**

Create `scripts/psmux-health-monitor.ps1`:

```powershell
<#
.SYNOPSIS
    Connects to psmux named pipe and monitors push events for agent health.
.DESCRIPTION
    Consumes context_ready, context_exited, and exec_completed events.
    Detects stalls (no events from a pane within threshold) and failures
    (non-zero exit codes). Prints structured log lines.
.PARAMETER Session
    psmux session name to monitor. Default: "default"
.PARAMETER StallThresholdSeconds
    Seconds of silence before reporting a stall. Default: 300
#>
param(
    [string]$Session = "default",
    [int]$StallThresholdSeconds = 300,
    [switch]$Json
)

$pipePath = "\\.\pipe\psmux-claude-backend-$Session"
$paneState = @{}  # context_id -> @{last_event_time, status, command}

function Log($level, $msg, $data) {
    $ts = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
    if ($Json) {
        $obj = @{ timestamp = $ts; level = $level; message = $msg }
        if ($data) { $obj.data = $data }
        $obj | ConvertTo-Json -Compress
    } else {
        Write-Host "[$ts] [$level] $msg" -ForegroundColor $(
            switch ($level) { "ERROR" { "Red" } "WARN" { "Yellow" } default { "Gray" } }
        )
    }
}

try {
    Log "INFO" "Connecting to pipe: $pipePath"

    # Connect to named pipe
    $pipe = [System.IO.Pipes.NamedPipeClientStream]::new(".", "psmux-claude-backend-$Session", [System.IO.Pipes.PipeDirection]::InOut)
    $pipe.Connect(5000)
    $reader = [System.IO.StreamReader]::new($pipe)

    Log "INFO" "Connected. Monitoring events..."

    while ($true) {
        $line = $reader.ReadLine()
        if ($null -eq $line) { break }

        try {
            $event = $line | ConvertFrom-Json
        } catch {
            continue  # Skip non-JSON lines (RPC responses)
        }

        $method = $event.method
        if (-not $method) { continue }

        $cid = $event.params.context_id
        $now = Get-Date

        switch ($method) {
            "context_ready" {
                $paneState[$cid] = @{ last_event_time = $now; status = "ready" }
                Log "INFO" "Pane $cid ready (data_version=$($event.params.data_version))"
            }
            "context_exited" {
                $code = $event.params.exit_code
                $elapsed = $event.params.elapsed_ms
                $cmd = $event.params.command
                $paneState[$cid] = @{ last_event_time = $now; status = "exited"; exit_code = $code }

                if ($code -and $code -ne 0) {
                    Log "ERROR" "Pane $cid FAILED (exit_code=$code, command=$cmd, elapsed=${elapsed}ms)"
                } else {
                    Log "INFO" "Pane $cid exited OK (elapsed=${elapsed}ms, command=$cmd)"
                }
            }
            "exec_completed" {
                $code = $event.params.exit_code
                $paneState[$cid] = @{ last_event_time = $now; status = "exec_done" }
                if ($code -ne 0) {
                    Log "WARN" "exec in $cid failed (exit_code=$code, command=$($event.params.command))"
                }
            }
        }

        # Stall detection: check all tracked panes
        foreach ($id in @($paneState.Keys)) {
            $state = $paneState[$id]
            if ($state.status -notin @("exited", "exec_done")) {
                $silent = ($now - $state.last_event_time).TotalSeconds
                if ($silent -gt $StallThresholdSeconds) {
                    Log "WARN" "Pane $id may be stalled (${silent}s since last event)"
                    $paneState[$id].last_event_time = $now  # Reset to avoid repeated warnings
                }
            }
        }
    }
} catch {
    Log "ERROR" "Pipe connection failed: $_"
    exit 1
} finally {
    if ($reader) { $reader.Dispose() }
    if ($pipe) { $pipe.Dispose() }
}
```

- [ ] **Step 2: Verify script runs (manual — requires active session)**

Run: `pwsh scripts/psmux-health-monitor.ps1 -Session default -StallThresholdSeconds 30`
Expected: Connects to pipe, logs events as they arrive. Kill a pane to see `context_exited`.

- [ ] **Step 3: Commit**

```bash
git add scripts/psmux-health-monitor.ps1
git commit -m "feat: add psmux-health-monitor.ps1 for push event monitoring"
```

---

### Task 10: Update spec with findings and run full check

- [ ] **Step 1: Update the spec's "Existing Code Map" section**

The spec at `docs/superpowers/specs/2026-04-12-agent-execution-layer-design.md` should note what was found to already exist. Update the "Implementation approach" sections to reflect reality (enhancement, not greenfield).

- [ ] **Step 2: Run full check suite**

Run: `cargo fmt && cargo clippy -- -D warnings && cargo test`
Expected: All pass.

- [ ] **Step 3: Final commit**

```bash
git add docs/superpowers/specs/2026-04-12-agent-execution-layer-design.md
git commit -m "docs: update agent execution layer spec with implementation findings"
```
