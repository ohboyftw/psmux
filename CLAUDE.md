# psmux — Terminal Multiplexer for Windows

## Project Overview
psmux is a Windows-native terminal multiplexer (tmux alternative) built in Rust.
It provides tmux-compatible commands and keybindings for Windows Terminal, PowerShell, and cmd.exe.
Binaries: `psmux`, `pmux`, `tmux` (all identical aliases).

## Tech Stack
- **Language**: Rust (stable toolchain via `rust-toolchain.toml`)
- **Target**: Windows 10/11 only (uses Win32 Console APIs)
- **Build**: `cargo build --release`
- **Test**: `cargo test`
- **Lint**: `cargo clippy -- -D warnings`
- **Format**: `cargo fmt --check`
- **Package managers**: Chocolatey (`packages/chocolatey/`), Cargo (`crates.io`), GitHub Releases

## Repository Structure
```
src/           — Rust source (main binary + library modules)
tests/         — Integration tests
scripts/       — PowerShell install/uninstall scripts
packages/      — Chocolatey packaging
.cargo/        — Cargo configuration
.github/       — CI/CD workflows
psmux.json     — Default configuration schema
```

## Key Conventions
- All public functions must have doc comments (`///`)
- Unsafe blocks require a `// SAFETY:` comment explaining the invariant
- New tmux-compatible commands go in `src/` following the existing command dispatch pattern
- Windows API calls use the `windows` crate; wrap raw FFI in safe abstractions
- Error handling: use `anyhow::Result` for binary, `thiserror` for library errors
- Tests: unit tests in `#[cfg(test)]` modules, integration tests in `tests/`
- CI must pass: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`

## tmux Compatibility Status
92 tmux-compatible commands fully implemented across 7 categories: session (16),
window (15), pane (16), copy/paste (10), config/keys (13), display/UI (9),
scripting/orchestration (7), plus 6 agent-specific commands (`wait-pane`, `run`,
`capture-pane --clean`, `--json` output, `@agent` metadata, warm pool).
Full scripting support including `send-keys`, `capture-pane`, `pipe-pane`,
`run-shell`, `source-file`, `bind-key`/`unbind-key`.

### Remote & Backend Features (ohboy-builds)
- **Remote tmux control mode**: `attach-remote`, `new-session-remote`, `list-sessions-remote` — connect to remote tmux sessions over SSH using `-CC` control mode
- **CustomPaneBackend**: JSON-RPC named pipe server (`CLAUDE_PANE_BACKEND_SOCKET` env var) for Claude Code's TeammateTool agent spawning protocol. Injects `CLAUDE_CODE_NO_FLICKER=1` for flicker-free agent rendering.
- **DCS passthrough**: `set -g allow-passthrough on` forwards DCS tmux passthrough sequences to the host terminal (e.g., for title setting, clipboard)

### Neovim / TUI Support (ohboy-builds)
- **Focus event passthrough**: VT parser tracks DECSET `?1004h`, server injects `\x1b[I`/`\x1b[O` on pane switch — enables neovim `:checktime`/autoread
- **Cursor style tracking**: VT parser tracks DECSCUSR (`CSI Ps SP q`), client restores cursor shape (block/bar/underline) on pane switch — neovim normal/insert mode indicators work correctly
- **Encoding-aware mouse**: `write_mouse_to_pty` respects `mouse_protocol_encoding()` — SGR for apps requesting `?1006h`, X10 normal for legacy apps
- **Bracketed paste**: VT parser tracks `?2004h`, passthrough preserved across pane switches
- **Pane title bars**: `set -g pane-border-status top|bottom` with `pane-border-format` — per-pane title display
- **Status desaturation**: Auto-dims status bar on window unfocus, configurable via `status-unfocused-style`

## Common Tasks
- **Build**: `cargo build` (debug) / `cargo build --release` (optimized)
- **Run**: `cargo run` or `cargo run -- new-session -s work`
- **Test single module**: `cargo test -- module_name`
- **Check before PR**: `cargo fmt && cargo clippy -- -D warnings && cargo test`
- **Chocolatey pack**: `cd packages/chocolatey && choco pack`

## Architecture Notes
- Session management uses named pipes for IPC between detached sessions and client
- Pane rendering uses Windows Console Virtual Terminal Sequences (VT100)
- Key prefix system mirrors tmux: Ctrl+b default, configurable via `~/.psmux.conf`
- Configuration parsing supports tmux `set -g` syntax
- Copy mode implements vim-like keybindings with scrollback buffer (2000 lines default)
- **CustomPaneBackend** (`src/backend/`): JSON-RPC over named pipes — `protocol.rs` (types), `pipe.rs` (listener), `dispatcher.rs` (routing)
- **Remote control mode** (`src/remote/`): SSH transport + tmux `-CC` parser + pane manager for rendering remote sessions locally
- **DCS passthrough** (`crates/vt100-psmux/`): VT parser hook/put/unhook DCS handlers with `PassthroughQueue` forwarding
- **VT terminal state tracking** (`crates/vt100-psmux/`): Focus reporting (`?1004h`), cursor style (DECSCUSR), mouse mode/encoding — all tracked in `screen.rs` with `state_diff()` for pane switching
- **Focus event injection** (`src/server/helpers.rs`): `send_focus_events()` sends `\x1b[I`/`\x1b[O` on pane switch when child has `?1004h` enabled

## Upstream Sync
At session start, check `.claude/upstream-pulse/state.json` — if `last_check` is >3 days old,
nudge: "Run `/upstream-pulse` to check for upstream changes."

## Swarm Backend Context
psmux can serve as the tmux spawn backend for Claude Code's TeammateTool on Windows.
See `.claude/skills/swarm-orchestrator/` for multi-agent patterns and
`tests/validate-swarm-backend.ps1` for backend compatibility validation.
Read `references/architecture.md` for the full system design.
- **Validate swarm backend**: `pwsh tests/validate-swarm-backend.ps1`

## Test Infrastructure
- `tests-rs/test_boundary_contracts.rs` — 60 tests: SGR/X10 encoding, coordinate translation, VT state machine, pane-switch handshakes, edge cases
- `tests-rs/test_feature_contracts.rs` — 33 tests: focus reporting, cursor style, mouse encoding, state diff contracts
- `tests-rs/test_vt100_mouse.rs` — VT100 mouse mode detection
- `tests/` — PowerShell integration tests (swarm backend, issue-specific)
- Total: ~1,100 tests across all targets
