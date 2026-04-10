# Pi Coding Agent Integration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make psmux a first-class backend for Pi coding agent's `PsmuxAdapter` by adding env vars, fixing `PSMUX_SESSION`, writing early discovery files, and enriching the JSON-RPC `list` response.

**Architecture:** Direct inline changes to 3 files. No new modules, no new dependencies. Builder functions gain a `session_name` parameter; `ContextInfo` gains 4 fields; discovery file is written at session init.

**Tech Stack:** Rust, serde/serde_json, Windows named pipes

---

### Task 1: Enrich `ContextInfo` struct (R6 — protocol)

**Files:**
- Modify: `src/backend/protocol.rs:204-208`

This task must come first because the contract tests in `tests-rs/test_pi_integration_contracts.rs` reference the new fields — they won't compile until this is done.

- [ ] **Step 1: Add fields to `ContextInfo`**

In `src/backend/protocol.rs`, replace the current `ContextInfo` struct:

```rust
#[derive(Debug, Serialize)]
pub struct ContextInfo {
    pub context_id: String,
    pub metadata: Option<AgentMetadata>,
}
```

With:

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

- [ ] **Step 2: Fix the list handler in `src/server/mod.rs`**

In `src/server/mod.rs` around line 5171, replace the `ContextInfo` construction inside `collect_backend_panes()`:

```rust
Node::Leaf(p) => {
    let meta = crate::backend::protocol::AgentMetadata::from_metadata_map(&p.metadata);
    out.push(crate::backend::protocol::ContextInfo {
        context_id: format!("%{}", p.id),
        metadata: meta,
    });
}
```

With:

```rust
Node::Leaf(p) => {
    let meta = crate::backend::protocol::AgentMetadata::from_metadata_map(&p.metadata);
    out.push(crate::backend::protocol::ContextInfo {
        context_id: format!("%{}", p.id),
        alive: !p.dead,
        cwd: p.spawn_cwd.as_ref().map(|c| c.to_string_lossy().into_owned()),
        title: if p.title.is_empty() { None } else { Some(p.title.clone()) },
        shell_name: p.shell_name.clone(),
        metadata: meta,
    });
}
```

- [ ] **Step 3: Run contract tests to verify they compile and pass**

Run: `cargo test --test test_pi_integration_contracts`

Expected: All 30+ tests PASS (they were previously blocked by missing fields).

- [ ] **Step 4: Run full test suite**

Run: `cargo test`

Expected: All existing tests still pass. The `ContextInfo` change is additive — no existing code constructs it outside of the list handler.

- [ ] **Step 5: Commit**

```bash
git add src/backend/protocol.rs src/server/mod.rs
git commit -m "feat(backend): enrich ContextInfo with alive, cwd, title, shell_name (R6)"
```

---

### Task 2: Add `PSMUX=1` and `PI_PANE_BACKEND_SOCKET` and `PSMUX_PANE_ID` to `set_tmux_env()` (R1, R3, R7)

**Files:**
- Modify: `src/pane.rs:991-1047`

- [ ] **Step 1: Add 3 new env vars in `set_tmux_env()`**

In `src/pane.rs`, in the `set_tmux_env()` function, add the following lines.

After line 1010 (`builder.env("TMUX_PANE", ...)`), add:

```rust
    // R7: psmux-branded pane identity, mirrors TMUX_PANE.
    builder.env("PSMUX_PANE_ID", format!("%{}", pane_id));
```

After line 1014 (`builder.env("PSMUX_SESSION", session_name)`), add:

```rust
    // R1: simple boolean detection for any tool — `if (process.env.PSMUX)`.
    builder.env("PSMUX", "1");
```

After line 1019-1020 (`builder.env("CLAUDE_PANE_BACKEND_SOCKET", ...)`), add:

```rust
    // R3: pi-canonical backend socket discovery — same pipe, pi-branded name.
    builder.env(
        "PI_PANE_BACKEND_SOCKET",
        crate::backend::pipe::pipe_path(session_name),
    );
```

- [ ] **Step 2: Verify build compiles**

Run: `cargo check`

Expected: Compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add src/pane.rs
git commit -m "feat(pane): add PSMUX, PI_PANE_BACKEND_SOCKET, PSMUX_PANE_ID env vars (R1, R3, R7)"
```

---

### Task 3: Fix `PSMUX_SESSION` in builder functions (R2)

**Files:**
- Modify: `src/pane.rs:1235-1458` (builder functions + call sites at ~200, ~376, ~460, ~764)

- [ ] **Step 1: Add `session_name` parameter to `build_command()`**

In `src/pane.rs`, change the signature of `build_command()` from:

```rust
pub fn build_command(
    command: Option<&str>,
    env_shim: bool,
    allow_predictions: bool,
) -> CommandBuilder {
```

To:

```rust
pub fn build_command(
    command: Option<&str>,
    env_shim: bool,
    allow_predictions: bool,
    session_name: &str,
) -> CommandBuilder {
```

Then replace all 4 occurrences of `builder.env("PSMUX_SESSION", "1")` inside `build_command()` (lines 1255, 1281, 1304, 1317) with:

```rust
builder.env("PSMUX_SESSION", session_name);
builder.env("PSMUX", "1");
```

- [ ] **Step 2: Add `session_name` parameter to `build_default_shell()`**

Change the signature from:

```rust
pub fn build_default_shell(
    shell_path: &str,
    env_shim: bool,
    allow_predictions: bool,
) -> CommandBuilder {
```

To:

```rust
pub fn build_default_shell(
    shell_path: &str,
    env_shim: bool,
    allow_predictions: bool,
    session_name: &str,
) -> CommandBuilder {
```

Replace the single `builder.env("PSMUX_SESSION", "1")` at line 1398 with:

```rust
builder.env("PSMUX_SESSION", session_name);
builder.env("PSMUX", "1");
```

- [ ] **Step 3: Add `session_name` parameter to `build_raw_command()`**

Change the signature from:

```rust
pub fn build_raw_command(raw_args: &[String]) -> CommandBuilder {
```

To:

```rust
pub fn build_raw_command(raw_args: &[String], session_name: &str) -> CommandBuilder {
```

Replace `builder.env("PSMUX_SESSION", "1")` at line 1452 with:

```rust
builder.env("PSMUX_SESSION", session_name);
builder.env("PSMUX", "1");
```

**Important:** The `build_raw_command` fallback on line 1441 calls `build_command(None, true, false)` when `raw_args` is empty. Update this to pass `session_name`:

```rust
return build_command(None, true, false, session_name);
```

- [ ] **Step 4: Update all call sites to pass `session_name`**

There are 4 call sites that need updating. Each already has `app.session_name` in scope.

**`create_window()` (~line 200-206):** Change:

```rust
    let mut shell_cmd = if command.is_some() {
        build_command(command, app.env_shim, app.allow_predictions)
    } else if !expanded_shell.is_empty() {
        build_default_shell(&expanded_shell, app.env_shim, app.allow_predictions)
    } else {
        build_command(None, app.env_shim, app.allow_predictions)
    };
```

To:

```rust
    let mut shell_cmd = if command.is_some() {
        build_command(command, app.env_shim, app.allow_predictions, &app.session_name)
    } else if !expanded_shell.is_empty() {
        build_default_shell(&expanded_shell, app.env_shim, app.allow_predictions, &app.session_name)
    } else {
        build_command(None, app.env_shim, app.allow_predictions, &app.session_name)
    };
```

**`spawn_warm_pane()` (~line 375-378):** Change:

```rust
    let mut shell_cmd = if !expanded_shell.is_empty() {
        build_default_shell(&expanded_shell, app.env_shim, app.allow_predictions)
    } else {
        build_command(None, app.env_shim, app.allow_predictions)
    };
```

To:

```rust
    let mut shell_cmd = if !expanded_shell.is_empty() {
        build_default_shell(&expanded_shell, app.env_shim, app.allow_predictions, &app.session_name)
    } else {
        build_command(None, app.env_shim, app.allow_predictions, &app.session_name)
    };
```

**`create_window_raw()` (~line 460):** Change:

```rust
    let mut shell_cmd = build_raw_command(raw_args);
```

To:

```rust
    let mut shell_cmd = build_raw_command(raw_args, &app.session_name);
```

**`split_active_with_command()` (~line 763-768):** Change:

```rust
    let mut shell_cmd = if command.is_some() {
        build_command(command, app.env_shim, app.allow_predictions)
    } else if !expanded_shell.is_empty() {
        build_default_shell(&expanded_shell, app.env_shim, app.allow_predictions)
    } else {
        build_command(None, app.env_shim, app.allow_predictions)
    };
```

To:

```rust
    let mut shell_cmd = if command.is_some() {
        build_command(command, app.env_shim, app.allow_predictions, &app.session_name)
    } else if !expanded_shell.is_empty() {
        build_default_shell(&expanded_shell, app.env_shim, app.allow_predictions, &app.session_name)
    } else {
        build_command(None, app.env_shim, app.allow_predictions, &app.session_name)
    };
```

- [ ] **Step 5: Search for any other call sites**

Run: `cargo check`

If there are additional call sites that now fail (e.g. in tests or other modules), add `session_name` parameter. The compiler will flag all of them.

- [ ] **Step 6: Run tests**

Run: `cargo test`

Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/pane.rs
git commit -m "fix(pane): pass real session_name to builder functions (R2)"
```

---

### Task 4: Write discovery file at session start (R4)

**Files:**
- Modify: `src/server/mod.rs:583-594`

- [ ] **Step 1: Add early `.pipe` file write**

In `src/server/mod.rs`, after line 589 (the `.version` file write) and before line 591 (the `PSMUX_TARGET_SESSION` env var), add:

```rust
    // R4: Write pipe discovery file early so PsmuxAdapter can detect psmux
    // before the pipe listener thread starts.  The pipe listener also writes
    // this file (idempotent update) once it starts accepting connections.
    let pipe_name = crate::backend::pipe::pipe_path(&app.session_name);
    let pipe_file = format!("{}\\{}.pipe", dir, app.port_file_base());
    let _ = std::fs::write(&pipe_file, &pipe_name);
```

- [ ] **Step 2: Verify build compiles**

Run: `cargo check`

Expected: Compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add src/server/mod.rs
git commit -m "feat(server): write .pipe discovery file at session start (R4)"
```

---

### Task 5: Final validation

**Files:** None (verification only)

- [ ] **Step 1: Run clippy**

Run: `cargo clippy -- -D warnings`

Expected: No warnings.

- [ ] **Step 2: Run format check**

Run: `cargo fmt --check`

Expected: No formatting issues.

- [ ] **Step 3: Run full test suite**

Run: `cargo test`

Expected: All tests pass, including the 30+ new Pi integration contract tests.

- [ ] **Step 4: Run contract tests specifically**

Run: `cargo test --test test_pi_integration_contracts -- --nocapture`

Expected: All 30+ tests pass with no output issues.

- [ ] **Step 5: Verify no regressions in existing contract tests**

Run: `cargo test --test test_boundary_contracts --test test_feature_contracts`

Expected: All existing boundary/feature contract tests still pass.
