# ohboy-builds Feature Registry

Living checklist of features exclusive to `ohboy-builds`. **Verify after every upstream merge.**

## How to Use

After cherry-picking or merging from `upstream/master`:
1. `cargo test` — catches compilation and unit regressions
2. `cargo clippy -- -D warnings` — catches lint regressions
3. Walk this checklist — verify each feature still works at integration level

Mark any broken feature with `BROKEN` and the commit that caused it.

---

## Feature Checklist

### 1. CustomPaneBackend (JSON-RPC named pipe server)
- **Files**: `src/backend/dispatcher.rs`, `src/backend/pipe.rs`, `src/backend/protocol.rs`
- **Env var**: `CLAUDE_PANE_BACKEND_SOCKET`
- **Test**: `pwsh tests/validate-swarm-backend.ps1`
- **Verify**: Start psmux, set env var, confirm named pipe listener starts, Claude Code TeammateTool can spawn panes
- **Risk from upstream**: Changes to pane lifecycle, `src/types.rs` (CtrlReq enum), `src/server/mod.rs` (command dispatch)

### 2. Remote tmux Control Mode (client-side)
- **Files**: `src/remote/mod.rs`, `src/remote/parser.rs`, `src/remote/protocol.rs`, `src/remote/ssh.rs`, `src/remote/pane_manager.rs`, `src/remote/octal.rs`
- **Commands**: `attach-remote`, `new-session-remote`, `list-sessions-remote`
- **Test**: `cargo test` (parser/protocol unit tests)
- **Verify**: `psmux attach-remote -t <session> user@host` renders remote panes locally
- **Risk from upstream**: Control mode protocol changes (Tier 1 — upstream now has server-side `-C`/`-CC`), `src/types.rs` changes

### 3. DCS Passthrough
- **Files**: `crates/vt100-psmux/src/perform.rs` (hook/put/unhook), `PassthroughQueue`
- **Config**: `set -g allow-passthrough on`
- **Test**: `cargo test --test passthrough_tests`
- **Verify**: DCS sequences forwarded to host terminal (title setting, clipboard)
- **Risk from upstream**: VT parser changes in `crates/vt100-psmux/`

### 4. Three-State Pane Focus Borders
- **Files**: `src/rendering.rs` (eff_border/eff_active), `src/app.rs` (status bar dim), `src/server/mod.rs` (window_focused toggle)
- **Config**: `set -g pane-border-unfocused-style "fg=darkgray,dim"`
- **Test**: `cargo test --test focus_border_tests`
- **Verify**: Alt-tab away from psmux — all borders dim. Alt-tab back — active pane border restores to green.
- **Risk from upstream**: Changes to `render_node` signature, border drawing in `src/rendering.rs`, FocusIn/FocusOut handlers

### 5. Session Resurrection
- **Files**: `src/resurrection.rs`
- **Commands**: `resurrect`, `delete-resurrect`, `list-sessions` (shows snapshots)
- **Verify**: Kill session, `psmux ls` shows snapshot, `psmux resurrect <name>` restores it
- **Risk from upstream**: Session/window lifecycle changes, AppState serialization

### 6. Agent Orchestration (warm pool, wait-pane, @agent metadata)
- **Commands**: `wait-pane`, `run`, `capture-pane --clean`, `--json` output, `@agent` user options
- **Verify**: `psmux wait-pane -t %0 -S ready` blocks until pane signals readiness
- **Risk from upstream**: Pane metadata, command dispatch, `src/server/connection.rs`

### 7. Hints Mode
- **Files**: `src/hints.rs`
- **Verify**: In copy mode, hints overlay works for URLs/paths
- **Risk from upstream**: Input handling, copy mode changes

### 8. Mycel Event Bus Integration
- **Files**: `src/mycel.rs`
- **Config**: Feature-gated (`feature = "mycel"`)
- **Risk from upstream**: Low (isolated module)

### 9. Popup-as-Pane Architecture
- **Files**: `src/popup.rs` (pane-backed popups)
- **Commands**: `display-popup` with `-w`, `-h`, `-d`, `-c` flags
- **Verify**: `psmux display-popup -- htop` opens a bordered popup running htop
- **Risk from upstream**: Popup rendering, pane lifecycle, window_ops

### 10. Enhanced Format Engine
- **Files**: `src/format.rs` (extended format variables, modifiers)
- **Verify**: `psmux display-message "#{pane_current_path}"` expands correctly
- **Risk from upstream**: Format variable additions (usually additive, low risk)

### 11. Pane Focus Visibility (Frame + Title Bar + Status Desaturation)
- **Files**: `src/rendering.rs` (title bar, frame border, draw_title_line helper), `src/app.rs` (status desaturation), `src/style.rs` (desaturate helpers)
- **Config**: `pane-border-status top|bottom|off`, `pane-border-format`, `status-unfocused-style`
- **Test**: `cargo test` + manual visual check
- **Verify**: Start psmux, verify title bar visible on panes, split panes to check multi-pane titles, alt-tab to check status desaturation, `set -g pane-border-status off` disables all
- **Risk from upstream**: Changes to `render_node` signature in `src/rendering.rs`, status bar rendering in `src/app.rs`, border drawing logic

### 12. VT Terminal State Tracking (Focus Reporting + Cursor Style)
- **Files**: `crates/vt100-psmux/src/screen.rs` (mode bits, accessors, state_diff), `crates/vt100-psmux/src/perform.rs` (DECSCUSR handler), `crates/vt100-psmux/src/term.rs` (FocusEvents struct)
- **Server wiring**: `src/server/helpers.rs` (`send_focus_events()`), `src/server/mod.rs` (FocusPane/FocusPaneCmd handlers)
- **Capabilities**:
  - `screen.focus_reporting()` — tracks DECSET ?1004h/?1004l
  - `screen.cursor_style()` — tracks DECSCUSR (CSI Ps SP q, values 0-6)
  - `state_diff()` emits both when switching panes
  - Focus events (\x1b[I / \x1b[O) injected on pane switch for neovim autoread
  - Cursor shape (block/bar/underline) restored on pane switch for neovim mode indicators
- **Test**: `cargo test --test test_feature_contracts` (33 tests), `cargo test --test test_boundary_contracts` (60 tests)
- **Verify**: Run neovim in a pane, split, switch panes — cursor shape should follow, `:checktime` should trigger on focus
- **Risk from upstream**: VT parser changes in `crates/vt100-psmux/`, DECSET handler additions

### 13. Encoding-Aware Mouse Protocol
- **Files**: `src/window_ops.rs` (`write_mouse_to_pty`)
- **Capabilities**:
  - Checks `mouse_protocol_encoding()` from pane's VT parser before emitting mouse events
  - SGR format (`\x1b[<btn;col;rowM/m`) when child requested ?1006h
  - X10 normal format (`\x1b[Mcbcxcy`) with coordinate clamping (max 222) and press-only semantics when child uses default encoding
  - Fixes pre-existing X10 coordinate overflow bug (`col as u8` wrapping for values > 255)
- **Test**: `cargo test --test test_boundary_contracts` (mouse encoding tests), `cargo test --test test_feature_contracts` (X10 clamping)
- **Verify**: Run neovim (SGR) and a legacy app (X10) — mouse clicks/scrolling should work in both
- **Risk from upstream**: Changes to mouse injection pipeline in `src/window_ops.rs`, `src/input.rs`

---

## Post-Merge Quick Smoke Test

```bash
# 1. Build + lint + test
cargo fmt --check && cargo clippy -- -D warnings && cargo test

# 2. Swarm backend validation
pwsh tests/validate-swarm-backend.ps1

# 3. Manual: start psmux, split panes, alt-tab, verify borders dim
cargo run -- new-session -s test
# Ctrl+b % (split), alt-tab away, alt-tab back

# 4. Manual: popup
# Inside psmux: Ctrl+b then run display-popup -- cmd
```

## Update Policy

- Add new features to this list when they land on ohboy-builds
- After each upstream merge, check the "Risk from upstream" column
- If a merge touches listed risk files, manually verify that feature
