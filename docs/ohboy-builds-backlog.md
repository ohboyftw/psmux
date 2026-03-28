# ohboy-builds Backlog

Persistent task backlog for the ohboy-builds fork. Updated after each `/upstream-pulse` run.

## Upstream Merge Strategy

| Category | Action | Example |
|----------|--------|---------|
| **Fixes** | Cherry-pick directly | Rendering bugs, config parsing, install script |
| **Non-overlapping features** | Merge/cherry-pick | New test files, docs, additive-only modules |
| **Overlapping features** | Decompose into backlog tasks | Features that touch files ohboy-builds has diverged on |

## How to Use

- After `/upstream-pulse`, categorize each item and either cherry-pick immediately or add to backlog
- Backlog items have status: `TODO`, `IN PROGRESS`, `DONE`, `WONT DO`
- Review backlog at session start or when planning sprints
- Mark items `WONT DO` with reason if ohboy-builds has a better approach

---

## Active Backlog

### Upstream Control Mode Server (-C/-CC) — `b68962b`

Upstream added server-side control mode (2,594 lines). ohboy-builds has CustomPaneBackend (JSON-RPC) for the same programmatic control use case, but control mode adds tmux protocol compatibility for third-party tooling.

**Decomposed tasks:**

- [ ] **Extract shared octal codec** `TODO`
  - ohboy-builds has `src/remote/octal.rs`, upstream adds encoding in `src/control.rs`
  - Action: Extract to `src/octal.rs` shared by both remote (client) and control (server)
  - Files: `src/remote/octal.rs` → `src/octal.rs`, update imports in `src/remote/mod.rs`
  - Risk: Low — pure utility extraction

- [ ] **Port output ring buffer to panes** `TODO`
  - Upstream adds 64KB per-pane output ring buffer in reader thread (`src/pane.rs`)
  - Useful for control mode notifications AND for capture-pane improvements
  - Files: `src/pane.rs` (additive, ~38 lines)
  - Risk: Low — additive to existing pane struct

- [ ] **Port `for_each_pane()` tree traversal** `TODO`
  - Small utility function added to `src/tree.rs` (~10 lines)
  - Useful for any feature that needs to visit all panes
  - Files: `src/tree.rs` (additive)
  - Risk: Low — additive helper

- [ ] **Add ControlNotification enum and ControlClient to types** `TODO`
  - Upstream adds ~59 lines to `src/types.rs`: notification types, client struct, CtrlReq variants
  - Files: `src/types.rs` — heavily diverged, needs manual port
  - Risk: Medium — types.rs is a conflict hotspot
  - Blocked by: shared octal codec extraction

- [ ] **Port `src/control.rs` core module** `TODO`
  - Server-side control mode formatting, escaping, notification emission (327 lines)
  - NEW file — no conflicts, but depends on types additions
  - Files: `src/control.rs` (create), `src/lib.rs` (add mod)
  - Risk: Low — new file
  - Blocked by: ControlNotification types, output ring buffer

- [ ] **Port CONTROL protocol handler in server/connection.rs** `TODO`
  - 683 lines of command dispatch for control mode clients
  - Files: `src/server/connection.rs` — heavily diverged
  - Risk: High — largest conflict surface, 40+ command handlers
  - Blocked by: control.rs, types, output ring buffer

- [ ] **Port notification emission in server/mod.rs** `TODO`
  - Hook events emit control mode notifications (~81 lines)
  - Files: `src/server/mod.rs` — heavily diverged
  - Risk: Medium — localized additions but in a large diverged file
  - Blocked by: control protocol handler

- [ ] **Add -C/-CC CLI flags** `TODO`
  - Wire control mode entry point in `src/main.rs` (~123 lines)
  - Includes `run_control_mode()` with graceful shutdown
  - Files: `src/main.rs` — diverged but CLI section is relatively stable
  - Risk: Medium
  - Blocked by: all control mode modules

- [ ] **Port control mode docs and tests** `TODO`
  - `docs/control-mode.md` (379 lines) — NEW, no conflicts
  - `tests/test_control_mode.ps1` (288 lines) — NEW, no conflicts
  - Risk: None — additive files
  - Blocked by: -C/-CC CLI flags (tests need working implementation)

### Tier 4 Upstream Fixes

- [ ] **Port layout serialization from `6538a6e`** `TODO`
  - Fast layout serialization added to `src/layout.rs` and `src/client.rs`
  - Strikethrough/hidden part already ported; layout serialization is new
  - Risk: Medium — layout.rs has diverged

- [ ] **Cherry-pick rendering test suite `262abc3`** `TODO`
  - 22 end-to-end rendering tests in new file `tests-rs/test_issue155_rendering.rs`
  - Additive, new file only — should apply clean
  - Risk: None

- [ ] **Port popup strikethrough test fixes `8359040`** `TODO`
  - Test registration fixes in `tests-rs/test_issue155_sgr_attrs.rs`
  - Cargo.toml test target additions
  - Risk: Low

---

## Completed

| Item | Date | Method |
|------|------|--------|
| Install script arch detection (`8cfabd8`) | 2026-03-29 | Cherry-pick clean |
| Bind-key case sensitivity (`6650a03`) | 2026-03-29 | Cherry-pick auto-merge |
| HIDDEN rendering workaround (`ee35684`) | 2026-03-29 | Cherry-pick + manual conflict resolution |
| --version fix (`87b6c28`) | 2026-03-29 | Skipped — ohboy-builds has better impl |
| run-shell tilde/XDG (`93ecdce`) | Earlier | Already ported in `b2874c5` |
| Strikethrough SGR (`c14a32e`) | Earlier | Already ported in `ecd37ad` |

## Won't Do

| Item | Reason |
|------|--------|
| (none yet) | |
