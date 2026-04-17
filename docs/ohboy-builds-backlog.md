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

### Tier 2: Quick Config (sync-2026-04-18-finch) — RESOLVED

- [x] **Default `allow-set-title` to off** — `WONT DO STANDALONE` (subsumed by Tier 3)
  - Upstream `4162d97` flips a default, but ohboy-builds never had the
    `allow_set_title` field — the flip is meaningless without first porting
    the feature. Merged into the Tier 3 port below (field added with
    default = `false`, matching post-flip upstream).

- [x] **Reset `rsel_pane_rect`/`rsel_block` in right-click copy path** — `WONT DO`
  - Upstream `e0657c4` references `rsel_*` state from PR #212 (pwsh-mouse-selection),
    which ohboy-builds has never ported. When/if PR #212 is ported (Tier 5,
    `4b5b5a4`), this fix should be included in the same port.

### Tier 3: Low Complexity / High Impact (sync-2026-04-18-finch)

- [ ] **Port `allow_set_title` field + OSC 0/2 pane_title propagation** `TODO`
  - Combines `eb338c8` (extract helper, run unconditionally) + `4162d97`
    (default to off) + the original inline OSC propagation that never
    landed on ohboy-builds.
  - Steps:
    1. Add `allow_set_title: bool` to `AppState` in `src/types.rs` (default `false`).
    2. Add `title_locked: bool` to `Pane` (for `select-pane -T` honoring).
    3. Add `propagate_osc_titles()` + `propagate_osc_titles_in_tree()` to `src/server/helpers.rs`.
    4. Call `propagate_osc_titles(&mut app)` in `server/mod.rs` pre-auto-rename pass (~L1186).
    5. Wire `set -g allow-set-title` parsing into `config.rs` + `server/options.rs`.
    6. Cherry-pick `docs/pane-titles.md` (additive, 175 lines).
    7. Port 7 verification tests from `tests/test_issue231_osc_title_propagation.ps1`.
  - Files: `src/types.rs`, `src/server/helpers.rs`, `src/server/mod.rs`,
    `src/config.rs`, `src/server/options.rs`, `docs/pane-titles.md`, tests.
  - Value: pairs with Feature #11 (Pane Title Bar) — OSC-emitting programs
    (ssh, vim, starship) can drive pane border titles automatically.
  - Risk: Medium — server/mod.rs + types.rs both diverged
  - REGRESSION RISK: CustomPaneBackend, Agent Orchestration

- [ ] **Window name no longer flashes to pwsh on session creation** `TODO`
  - Upstream: `b6c7784` — `src/commands.rs` +16, `src/platform.rs` +10, `src/server/connection.rs` +14, `src/server/mod.rs` +13
  - Value: cleaner agent spawn UX (no pwsh flash for CustomPaneBackend jobs)
  - Risk: Medium — 4 diverged files
  - REGRESSION RISK: CustomPaneBackend

- [ ] **Fix stale .port files when pane spawn fails (#204)** `TODO`
  - Upstream: `fd07145` — `src/server/mod.rs` (~28 lines) + new test
  - Value: directly relevant to CustomPaneBackend pipe-discovery hygiene
  - Risk: Medium — diverged file
  - REGRESSION RISK: CustomPaneBackend

### Tier 5: Cross-Session join-pane via TCP Proxy — `f8fa11d` + `85cafd9`

Upstream added a new multi-session transport layer for join-pane/move-pane with a
TCP proxy. Overlaps conceptually with CustomPaneBackend (both offer "pane as
transport endpoint") but via different plumbing.

**Decomposed tasks:**

- [ ] **Port `src/cross_session.rs` (161 lines, NEW)** `TODO`
  - Additive file — cherry-pick clean unless imports conflict
  - Risk: Low

- [ ] **Port `src/cross_session_server.rs` (302 lines, NEW)** `TODO`
  - Additive file — TCP listener for cross-session pane proxying
  - Risk: Low

- [ ] **Port `src/proxy_pane.rs` (276 lines, NEW)** `TODO`
  - Additive file — proxy pane representation
  - Risk: Low

- [ ] **Wire cross-session types into `src/types.rs`** `TODO`
  - Upstream adds ~49 lines to types.rs; ohboy-builds has CtrlReq divergence
  - Risk: Medium — conflict hotspot; WONT DO if CustomPaneBackend already
    subsumes the use case

- [ ] **Wire cross-session handlers into server/connection.rs + server/mod.rs** `TODO`
  - server/connection.rs +96, server/mod.rs +51, main.rs +111
  - Risk: High — all heavily diverged
  - Consider: skip if CustomPaneBackend's JSON-RPC `exec` + pipe protocol
    covers the target use case (likely yes for agent workloads)

### Tier 5: CREATE_NO_WINDOW + Combined Flag Parsing — `a5d1b23`

16k-line commit combines three independent features + a massive test suite.

**Decomposed tasks:**

- [ ] **Port CREATE_NO_WINDOW for background subprocesses** `TODO`
  - Files: `src/platform.rs` (+54 additive), `src/copy_mode.rs`, a few other spawn sites
  - Value: eliminates pwsh/cmd flash on agent spawn (critical for CustomPaneBackend UX)
  - Risk: Medium — `src/platform.rs` is a known diverged file
  - Priority: high for agent UX polish

- [ ] **Port combined flag parsing (e.g., `-abc` → `-a -b -c`)** `TODO`
  - Files: `src/commands.rs`, `src/server/connection.rs`, `src/main.rs`
  - Risk: Medium — commands.rs heavily diverged

- [ ] **Port `set-option -o` (only-if-unset)** `TODO`
  - Small addition to `src/server/options.rs` / option parsing
  - Risk: Low — localized

- [ ] **Cherry-pick rust test files (additive)** `TODO`
  - `tests-rs/test_flag_parity.rs` (2171 lines), `tests-rs/test_config_exhaustive.rs`
    (2545 lines), `tests-rs/test_hide_window.rs` (419), `tests-rs/test_mega_unit_coverage.rs`
    (715), `tests-rs/test_issue215_session_persistence.rs` (511)
  - Risk: Low if they compile; may need adaptation for ohboy-builds divergence

- [ ] **Cherry-pick PowerShell integration tests (additive)** `TODO`
  - test_cli_flag_parity, test_cli_mega_suite, test_combined_flags,
    test_config_exhaustive_{cli,tcp,tui}, test_hide_window_e2e,
    test_issue215_session_persistence, test_tcp_*, test_win32_tui_*
  - Risk: Low — additive, but may reference upstream-only codepaths

### Tier 5: Remove legacy `src/app.rs` — `44de600`

- [ ] **Evaluate whether to follow upstream deletion of `src/app.rs`** `TODO` `LOW`
  - Upstream deleted `src/app.rs` (1303 lines) and migrated functionality into
    client/server modules
  - ohboy-builds still uses `src/app.rs` for our own additions (bracket paste
    state, run-shell, focus-events wiring)
  - Risk: Very high — would be a multi-day refactor
  - Likely resolution: WONT DO (follow upstream) or LATER (wait for
    ohboy-builds to independently converge on client/server split)

### Tier 5: Misc Features (sync-2026-04-18-finch)

- [ ] **pane_title defaults to hostname + show-options resolves default-shell** `TODO`
  - Upstream: `e20630b` — `src/pane.rs` +13, `src/format.rs` +29, server/mod.rs +35
  - Plus 18 new PowerShell tests + `tests/injector.cs` (213-line keystroke injector)
  - Risk: Medium — pane.rs and format.rs diverged
  - Decompose: take the injector helper + test files separately (additive),
    port the default-shell resolution logic to show-options

- [ ] **new-session -e environment variable support (#205)** `TODO`
  - Upstream: `9926b85` + `38d7cfa` — `src/commands.rs`, `src/main.rs`,
    `src/server/connection.rs`, `src/server/mod.rs`, `src/util.rs` (+139 NEW file)
  - Value: clean way to set per-session env — useful for agent teams
    (CLAUDE_PANE_BACKEND_SOCKET per spawn)
  - Risk: Medium — 4 diverged files + 1 new util.rs file
  - REGRESSION RISK: CustomPaneBackend, Agent Orchestration

- [ ] **pwsh-mouse-selection option (#211)** `TODO` `LOW`
  - Upstream: `4b5b5a4` — `src/client.rs` (+452) + options wiring
  - Value: interactive UX polish, low agent relevance
  - Risk: Medium — client.rs heavily diverged

- [ ] **send-keys C-x/M-x local path parsing aligned with server (#230)** `TODO`
  - Upstream: `d47b65c` — `src/commands.rs` +101
  - Value: fixes a send-keys correctness gap
  - Risk: Medium — commands.rs diverged
  - REGRESSION RISK: Agent Orchestration (send-keys is core dispatch path)

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

### Tier 3: Hook Background Mode (sync-2026-04-11-wren) — DONE

- [x] **Port `ensure_background()` + `fire_hooks()` from `5f8c20d`** `DONE`

### Tier 4: Hook Quoting Fixes (sync-2026-04-11-wren) — DONE

- [x] **Port set-hook quoting preservation from `f81632a` + `3bcff11`** `DONE`
  - Client-side: not needed (ohboy-builds forwards raw cmd string to server)
  - Server-side: ported quoting-aware extraction + `-a` append support to `connection.rs`

### Tier 2: Flag Normalization (sync-2026-04-10-panther)

- [ ] **Port `-x=VALUE` flag normalization from `0b18080`** `TODO`
  - Tools like claude-squad use `has-session -t=NAME` which silently passes
  - Add `normalize_flag_equals()` to cli.rs, wire into app.rs + connection.rs entry points
  - Files: `src/cli.rs` (new function), `src/app.rs` (+1), `src/server/connection.rs` (+2) — all diverged
  - Test: `tests-rs/test_issue196_flag_equals.rs` (NEW, cherry-pick clean)
  - Risk: Low — flag parsing is upstream of dispatch

### Tier 3: Scroll Without Copy Mode (sync-2026-04-10-panther)

- [ ] **Port scroll_pane_scrollback from `6bcb9f5` (PR #194)** `TODO`
  - Scroll pane buffer directly when scroll-enter-copy-mode is off
  - Extract `scroll_pane_scrollback()` from copy_mode.rs, wire into input.rs + window_ops.rs
  - Add `scroll-enter-copy-mode` option to `src/server/options.rs` (option_catalog.rs deleted in ohboy-builds)
  - Files: `src/copy_mode.rs`, `src/input.rs`, `src/window_ops.rs`, `src/server/options.rs` — all diverged
  - Risk: Medium — touches input handling

### Tier 4: Fixes (sync-2026-04-10-panther)

- [ ] **Port defaults_suppressed reset from `266d414`** `TODO`
  - `unbind-key -a` flag never reset on source-file reload
  - Files: `src/commands.rs`, `src/server/mod.rs` — both diverged
  - Risk: Low

- [ ] **Port paste timeout from `86a7519`** `TODO`
  - Bracketed paste state machine has no timeout — lost close sequence = permanent freeze
  - Add paste_start timestamp, 2s timeout, 1MB cap
  - Files: `src/app.rs`, `src/ssh_input.rs` — both diverged
  - Risk: Medium — input handling sensitive

- [ ] **Port unbind-key -a table support from `f9d443d`** `TODO`
  - unbind-key -a now supports -T table flag; defaults_suppressed moved in struct
  - Files: `src/commands.rs`, `src/config.rs`, `src/pane.rs`, `src/server/connection.rs`, `src/server/mod.rs` — all diverged
  - New files: `examples/enter_diag.rs`, `tests/test_full_feature.ps1` (cherry-pick clean)
  - Risk: High — touches 5 diverged files

- [ ] **Verify session.rs namespace functions from `3e61a0d`** `TODO`
  - Check if `resolve_last_session_name_ns()` and `list_session_names_ns()` exist in ohboy-builds
  - If missing, port from upstream session.rs
  - Risk: Low — check only

### Tier 5: Docs (sync-2026-04-10-panther)

- [ ] **Merge upstream docs update from `c6088d5`** `TODO`
  - 13 features documented, 7 plugins, 4 themes added
  - Files: `docs/configuration.md`, `docs/features.md`, `docs/plugins.md`, `docs/scripting.md`
  - Risk: Low — docs only, may have minor conflicts

### Tier 2: Quick Config (sync-2026-04-01-griffin)

- [x] **Set `CLAUDE_CODE_NO_FLICKER=1` in CustomPaneBackend spawn env** `DONE`
  - Already implemented in `src/backend/dispatcher.rs:137-141`

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

### Tier 3: High-Impact Fixes (sync-2026-04-07-mantis)

- [x] **Port zoom meta_dirty fix from `acb51b2`** `DONE`
  - Status bar doesn't update `window_zoomed_flag` after zoom toggle
  - 1-line fix: add `meta_dirty = true` to ZoomPane handler in server/mod.rs

- [x] **Port run-shell absolute paths from `b9a6d88`** `DONE`
  - `resolve_run_shell()` returns full paths via which::which() + SystemRoot/COMSPEC fallback

- [x] **Port if-shell quote-aware parser from `b705280`** `N/A`
  - ohboy-builds forwards if-shell to server via control port; no local execution path to fix

### Tier 4: Fixes (sync-2026-04-07-mantis)

- [ ] **Port resize direction fix from `a8e3852`** `TODO`
  - Resize-pane moves border wrong direction when active pane is on bottom/right edge
  - Swap resize logic for fallback path in `resize_pane_vertical`/`resize_pane_horizontal`
  - Files: src/window_ops.rs (+18/-8), src/server/mod.rs (+1)
  - Risk: Medium — resize logic sensitive
  - Test: tests-rs/test_issue81_resize_direction.rs (NEW, 127 lines)

- [ ] **Port split-window MRU pollution fix from `d8c4d01`** `TODO`
  - `split-window -t` touches target pane's MRU rank, causing kill-pane to pick wrong next pane
  - Remove split-window from is_focus_cmd, add `focus_pane_by_id_no_mru()`, prev-by-index fallback
  - Files: src/pane.rs (+24/-2), src/server/connection.rs (+2/-4), src/server/mod.rs (+5/-1), src/tree.rs (+13/-2)
  - Risk: Medium — pane lifecycle changes, 4 diverged files
  - Test: tests/test_pane_mru.ps1 (NEW, 150 lines)

- [ ] **Port Shift+Enter 4 bugs from `cfb71bc`** `TODO`
  - Phantom CONTROL modifier, WezTerm Release-only, VT vs native injection split, phantom Release dedup
  - Large refactor: 194 insertions across 4 diverged files
  - Files: src/app.rs (+32), src/client.rs (+53), src/input.rs (+101/-46), src/platform.rs (+22/-2)
  - Risk: High — input handling is the most sensitive subsystem
  - Test: tests-rs/test_input.rs (+32)

### Tier 4: Interactive-Only Bug Fixes (sync-2026-04-06-lynx) — LOW PRIORITY

These are quality-of-life fixes for interactive tmux-compatible usage. They don't affect agent teams, swarm backend, or the primary ohboy-builds use cases. Port when convenient or when shipping psmux as a general-purpose multiplexer.

- [ ] **Port window-name targeting from `07bab22`** `TODO` `LOW`
  - Resolve window by name in `-t session:window_name`
  - Files: app.rs, cli.rs, server/connection.rs, server/mod.rs, types.rs
  - Risk: Medium — multiple diverged files
  - Impact: Agent teams use `-t %N` pane IDs, not window names

- [ ] **Port if-shell -F format expansion from `c4b2526`** `TODO` `LOW`
  - Format variables not expanded in if-shell -F conditions
  - 3 code paths: config.rs (parse_if_shell), main.rs (CLI), server/connection.rs (TCP)
  - Risk: Medium
  - Impact: Only matters if .psmux.conf uses format conditionals

- [ ] **Port run-shell double-wrapping fix from `eb8e468`** `TODO` `LOW`
  - run-shell adds extra shell wrapper when command already starts with shell binary
  - Fix: detect shell binary prefix, skip wrapping
  - Files: commands.rs, config.rs, main.rs, server/connection.rs
  - Risk: Medium
  - Impact: Edge case — env shim handles agent spawn path

- [ ] **Port run-shell async from `a510f7c`** `TODO` `LOW`
  - Make run-shell async to prevent UI freeze
  - Design: mpsc channel on AppState, spawn thread, drain in event loops
  - Files: app.rs, commands.rs, server/mod.rs, types.rs
  - Risk: Medium — adds RunShellOutput type
  - Impact: Agent teams use send-keys not run-shell

- [ ] **Port bind-key command mode fix from `9629068`** `TODO` `LOW`
  - bind-key/unbind-key/set-option from command mode silently dropped
  - Files: MANY — app.rs, commands.rs, config.rs, main.rs, server/connection.rs, server/mod.rs, types.rs
  - Risk: High — largest changeset, touches command dispatch pipeline
  - Impact: Only affects interactive `:bind-key` at runtime

- [ ] **Port resize-pane/split-window/layout fixes from `37ae071`** `TODO` `LOW`
  - resize-pane -x/-y, split-window -l, select-layout tiled
  - Design: (1) resize_all_panes after absolute/percent resize, (2) SplitWindow (u16,bool) for cells vs percent, (3) resize_all_panes after layout selection
  - Files: main.rs, server/connection.rs, server/mod.rs, tree.rs, types.rs
  - Risk: Medium — type changes + layout calculations
  - Impact: Swarm auto-layouts, rarely manually resized

### Tier 5: Layout Directives Feature (sync-2026-04-06-lynx)

- [ ] **Port #[align], #[fill], #[list], #[range] directives from `e584cbe`** `TODO`
  - Layout directives for status-format[] — flexible status bar layouts
  - Files: style.rs (+452), client.rs (+38/-22), format.rs (+16/-), new test (280 lines)
  - Risk: High — large feature, style.rs heavily diverged
  - Blocked by: bg=default fix (65f6611) should go first

- [ ] **Port pane title in border format from `0703b2b`** `TODO`
  - title_locked, meta_dirty, #{pane_title} in pane-border-format, client JSON fields
  - Files: app.rs, client.rs, layout.rs, pane.rs, popup.rs, rendering.rs, server/mod.rs, types.rs
  - Risk: High — touches rendering.rs (diverged for pane-border-status)
  - REGRESSION RISK: Pane Focus Visibility, Three-State Pane Focus Borders
  - Note: High value for ohboy-builds since pane-border-format is already implemented

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
| Env shim always-active (`467b32f`) | 2026-04-06 | Manual port — pane.rs + util.rs |
| ConPTY auto-retry error 87 (`1861eb7`) | 2026-04-06 | Cherry-pick clean |
| bg=default terminal color (`65f6611`) | 2026-04-06 | Manual port — style.rs |
| manual_rename on new-window -n (`7bcbecc`) | 2026-04-06 | Manual port — server/mod.rs |
| run-shell popup output (`d895d4a`) | 2026-04-06 | Pre-existing in working tree |
| bind-key new-window -c (`64ad68a`) | 2026-04-06 | Already fixed in ohboy-builds |
| Clippy fixes (perform.rs, term.rs, window_ops.rs) | 2026-04-06 | Direct fix |
| Zoom meta_dirty (`acb51b2`) | 2026-04-07 | Manual port — server/mod.rs |
| Run-shell absolute paths (`b9a6d88`) | 2026-04-07 | Manual port — commands.rs |
| Flag normalization -x=VALUE (`0b18080`) | 2026-04-10 | Manual port — cli.rs + 3 entry points |
| Paste timeout (#197) (`86a7519`) | 2026-04-10 | Manual port — app.rs + ssh_input.rs |
| CLAUDE_CODE_NO_FLICKER=1 | 2026-04-10 | Already present in dispatcher.rs |
| Hook run-shell background (`5f8c20d`) | 2026-04-11 | Manual port — commands.rs + server/mod.rs |
| Set-hook quoting (`f81632a` + `3bcff11`) | 2026-04-11 | Manual port — connection.rs (client-side N/A) |

## Won't Do

| Item | Reason |
|------|--------|
| Options catalog `option_catalog.rs` from `3ffc570` | ohboy-builds uses `src/server/options.rs` with its own approach; upstream file deleted in ohboy-builds |
| if-shell quote parser `b705280` | ohboy-builds forwards if-shell to server via control port; no local execute_command_string path to fix |
