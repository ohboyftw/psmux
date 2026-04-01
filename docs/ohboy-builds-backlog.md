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

### Tier 2: Quick Config (sync-2026-04-01-griffin)

- [ ] **Set `CLAUDE_CODE_NO_FLICKER=1` in CustomPaneBackend spawn env** `TODO`
  - Claude Code v2.1.89 adds flicker-free alt-screen rendering env var
  - Action: Add to pane spawn environment in `src/backend/dispatcher.rs`
  - Files: `src/backend/dispatcher.rs` (~1 line)
  - Risk: None — additive env var

### Tier 3: Manual Ports (sync-2026-04-01-griffin)

- [ ] **Port PredictionViewStyle fix from `e2f2b45`** `TODO`
  - Warm pane spawned before load_config() never gets allow-predictions setting
  - Action: After load_config(), check allow_predictions, respawn warm pane with correct PSReadLine init string
  - Files: `src/server/mod.rs` (config load path), `src/pane.rs` (init string logic)
  - Risk: Medium — server/mod.rs diverged; verify CustomPaneBackend after port
  - REGRESSION RISK: CustomPaneBackend, Agent Orchestration

- [ ] **Port status-right/left empty clearing from `ae47fee`** `TODO`
  - Format conditionals resolving to empty string leave stale content on screen
  - Action: Remove is_empty() guard in client status update path
  - Files: `src/client.rs` (~3 lines)
  - Risk: Low — targeted change in diverged file

### Tier 4: Fix Ports (sync-2026-04-01-griffin)

- [ ] **Port bell/silence flags clearing from `a2f853c`** `TODO`
  - check_window_activity() only cleared activity_flag, not bell_flag or silence_flag
  - Action: Clear all three flags for active window in server/helpers.rs
  - Files: `src/server/helpers.rs` (~5 lines)
  - Risk: Low

- [ ] **Port status-format[] inline styles from `b9be26a`** `TODO`
  - Lines 1+ used Span::styled() instead of parse_inline_styles()
  - Action: Use parse_inline_styles() for all status lines; honor status-format[0] override
  - Files: `src/client.rs` (~10 lines)
  - Risk: Low

- [ ] **Check nesting prevention from `3f95642`** `TODO`
  - Verify ohboy-builds has PSMUX_ACTIVE + PSMUX_SESSION guards in new-session and attach paths
  - If missing, port from upstream main.rs
  - Files: `src/main.rs`
  - Risk: Low — check only, port if needed

- [ ] **Check VTI/warmup/kill-server from `e29b954`** `TODO`
  - Three independent fixes: disable_vti_on_stdin(), warmup command, parallel kill-server
  - Verify each exists in ohboy-builds; port any missing
  - Files: `src/main.rs`
  - Risk: Low — check only, port if needed

### Tier 5: Feature Decomposition — 3ffc570 (sync-2026-04-01-griffin)

- [ ] **Port new format variables from `3ffc570`** `TODO`
  - pane_in_mode, cursor_x/y, pane_at_top/bottom/left/right, session_group, window_active_sessions
  - Files: `src/format.rs` — diverged, needs manual port
  - Risk: Medium — format.rs has significant divergence

- [ ] **Port layout improvements from `3ffc570`** `TODO`
  - Percentage split handling, layout checksum, custom layout string parsing
  - Files: `src/layout.rs` — diverged
  - Risk: Medium

- [ ] **Port mouse protocol support from `3ffc570`** `TODO`
  - SGR/normal mouse protocol, focus events, bracketed paste passthrough
  - Files: `src/input.rs` — diverged
  - Risk: Medium — input handling is sensitive

- [ ] **Port pane enhancements from `3ffc570`** `TODO`
  - respawn-pane, pipe-pane improvements, pane_dead state, remain-on-exit
  - Files: `src/pane.rs` — diverged
  - Risk: Medium

- [ ] **Port client enhancements from `3ffc570`** `TODO`
  - refresh-client, resize-window, enhanced display-message formats
  - Files: `src/client.rs` — diverged
  - Risk: Medium

- [ ] **Port VT100 improvements from `3ffc570`** `TODO`
  - OSC title capture, improved cursor restore
  - Files: `crates/vt100-psmux/src/perform.rs`, `screen.rs` — diverged
  - Risk: Medium — DCS Passthrough risk

- [ ] **Cherry-pick test files from `3ffc570`** `TODO`
  - `tests-rs/test_layout.rs` and `tests-rs/test_parity.rs` are new files
  - May need adaptation for ohboy-builds divergence
  - Risk: Low — new files but may reference upstream-only code

- [ ] **Port status-format[] tests from `8aa4ec2`** `TODO`
  - End-to-end tests for inline styles in style.rs, test_client.rs, test_format.rs
  - Blocked by: b9be26a inline styles port
  - Risk: Low

### Tier 4 Upstream Fixes (sync-2026-03-29-raven)

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
| Options catalog `option_catalog.rs` from `3ffc570` | ohboy-builds uses `src/server/options.rs` with its own approach; upstream file deleted in ohboy-builds |
