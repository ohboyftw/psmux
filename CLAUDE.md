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
- **CustomPaneBackend**: JSON-RPC named pipe server (`CLAUDE_PANE_BACKEND_SOCKET` / `PI_PANE_BACKEND_SOCKET` env vars) for Claude Code's TeammateTool and Pi coding agent's PsmuxAdapter. Injects `CLAUDE_CODE_NO_FLICKER=1` for flicker-free agent rendering.
- **Pi coding agent integration**: `PSMUX=1` boolean detection, `PSMUX_SESSION` set to real session name in all code paths, `PSMUX_PANE_ID` mirrors `TMUX_PANE`, enriched JSON-RPC `list` response (`alive`, `cwd`, `title`, `shell_name`), early `~/.psmux/{session}.pipe` discovery file write at session start
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
- **Agent Execution Layer** (`src/backend/dispatcher.rs`, merged into `ohboy-builds`): Programmatic-first agent interface. JSON-RPC `exec` method runs a command directly in a target pane (`-t %N`), returns PID + exit code. Push events: `context_ready` (pane reached idle prompt), `exec_completed` (command finished with exit_code + elapsed_ms), and enriched `context_exited` (adds elapsed_ms + command). CLI mirrors: `psmux exec -t %N -- command`, `new-window -- command`, `split-window -- command`. **Raw-argv mode** (`new-window --raw --`) bypasses the default-shell wrapper and spawns `argv[0]` directly — avoids pwsh intercepting `>` / `|` / `&&` before the inner command sees them. Full JSON-RPC protocol reference: [docs/custompane-backend.md](docs/custompane-backend.md).
- **Remote control mode** (`src/remote/`): SSH transport + tmux `-CC` parser + pane manager for rendering remote sessions locally
- **DCS passthrough** (`crates/vt100-psmux/`): VT parser hook/put/unhook DCS handlers with `PassthroughQueue` forwarding
- **VT terminal state tracking** (`crates/vt100-psmux/`): Focus reporting (`?1004h`), cursor style (DECSCUSR), mouse mode/encoding — all tracked in `screen.rs` with `state_diff()` for pane switching
- **Focus event injection** (`src/server/helpers.rs`): `send_focus_events()` sends `\x1b[I`/`\x1b[O` on pane switch when child has `?1004h` enabled
- **Mycel event bus** (`src/mycel.rs`): Publishes `psmux/*` topics when built with `--features mycel`. Full topic list: [docs/mycel-topics.md](docs/mycel-topics.md).
- **Server-side wait** (`src/wait_for.rs`): `psmux wait-for` blocks until a condition is met — `--exit PID` (WaitForSingleObject), `--file PATH` (filesystem poll), `--output REGEX` (live screen buffer match), `--ready` (pane idle prompt). **`--timeout` is milliseconds** (both `wait-for` and `wait-pane`); CLI socket `read_timeout` is sized from it. Exit codes: `0` success, `1` timeout, `2` error. JSON-RPC `wait_for` method in CustomPaneBackend. `--json` flag for machine-readable `WaitOutcome`. Replaces all client-side polling loops.
- **DAG orchestration** (`src/orchestrate.rs`): `psmux orchestrate <plan.json>` reads a worker DAG, provisions git worktrees, launches panes in topological order (via `new-window --raw` — no shell wrapping), polls for exit, skips dependents on failure. Sets `remain-on-exit on` at session level before spawning so dead panes survive the 500ms poll tick and their exit codes are recoverable; explicitly kills them after observation. State persisted to `<plan_dir>/.orchestration/<session>/state.json` for crash recovery. Flags: `--timeout <ms>` (hard ceiling, exit code `3`), `--cleanup` (remove worktrees), `--json` (machine output), `--session <name>` (override). Sentinels: `EXIT_PANE_GONE (-1)` vs `EXIT_SESSION_GONE (-3)` — only per-pane case triggers crash-dump attribution. Full guide: [docs/orchestrate.md](docs/orchestrate.md).
- **Crash diagnostics** (`src/crash.rs`): Panic hook writes crash reports with backtrace to `%LOCALAPPDATA%/psmux/crashes/`. `psmux debug crashes list|show`. Auto-prunes to 20 newest on server start. Orchestrate records `crash_dump_path` in worker state when a pane dies without clean exit.

## Upstream Sync
At session start, check `.claude/upstream-pulse/state.json` — if `last_check` is >3 days old,
nudge: "Run `/upstream-pulse` to check for upstream changes."

## Internal Docs (`.claude/internal/`)

Sync trackers, planning docs, session checkpoints, internal requirements, and
the Porting Guardrails / Known Ohboy-Only Extensions / Incident-log tables live
in **`.claude/internal/`** as a **nested private git repo**, not in `docs/`.

- Outer psmux repo has `.claude/internal/` in `.gitignore` — these files never
  leak to the public GitHub.
- The nested repo has its own `git log`, branches, and (optionally) a private
  remote. Commit changes from inside the directory:
  `cd .claude/internal && git add FILE && git commit -m "..."`.
- Do **NOT** `git add` internal docs from the outer repo — it's ignored and has no effect.
- Claude Code reads `.claude/internal/*.md` exactly like any other file — fully visible in-session.
- Key files to consult at sync time:
  - `.claude/internal/ohboy-builds-backlog.md` — upstream sync ledger + Porting Guardrails + Known Ohboy-Only Extensions table + Incident log
  - `.claude/internal/on-rails-checklist.md` — current rails-sync status
  - `.claude/internal/session-checkpoint-*.md` — session-compaction handoffs

Only user- and developer-facing docs live in `docs/` and ship with the public release.

## Swarm Backend Context
psmux can serve as the tmux spawn backend for Claude Code's TeammateTool on Windows.
See `.claude/skills/swarm-orchestrator/` for multi-agent patterns and
`tests/validate-swarm-backend.ps1` for backend compatibility validation.
Read `references/architecture.md` for the full system design.
- **Validate swarm backend**: `pwsh tests/validate-swarm-backend.ps1`

## Test Infrastructure
- **Lib tests** (`cargo test --lib`): 113 tests covering wait_for, orchestrate, routing, tree, mycel, remote ssh, etc.
- `tests-rs/test_boundary_contracts.rs` — 60 tests: SGR/X10 encoding, coordinate translation, VT state machine, pane-switch handshakes, edge cases
- `tests-rs/test_feature_contracts.rs` — 40 tests: focus reporting, cursor style, mouse encoding, state diff contracts
- `tests-rs/test_pi_integration_contracts.rs` — 32 tests: Pi adapter env var contracts, ContextInfo schema/serialization, pipe discovery, edge cases
- `tests-rs/test_vt100_mouse.rs` — VT100 mouse mode detection
- **Rails bench** (`pwsh tests/run_rails_bench.ps1`): 14 per-feature ps1 files covering wait-for, orchestrate (×3 — core + timeout exit code 3 + resume-from-state.json), respawn-pane `-k`, capture-pane clamp, crash diagnostics, CREATE_NO_WINDOW, set-option `-o` guard, show-options `-gv`, source-file reload, status-format[0] array key, MIN_SPLIT_ROWS rejection. Wired into GitHub Actions CI on push.
- `tests/` — PowerShell integration tests (swarm backend, issue-specific)
- Total: ~1,140 tests across all targets

## User-Facing Documentation

| Topic | File |
|-------|------|
| Orchestration DAG runner | [docs/orchestrate.md](docs/orchestrate.md) |
| CustomPaneBackend JSON-RPC protocol | [docs/custompane-backend.md](docs/custompane-backend.md) |
| Remote tmux control over SSH | [docs/remote-tmux.md](docs/remote-tmux.md) |
| Scripting (exec, wait-for, wait-pane, --raw, format vars) | [docs/scripting.md](docs/scripting.md) |
| Configuration options + guarded set (`-o`/`-u`) + array-index keys | [docs/configuration.md](docs/configuration.md) |
| Mycel topic list | [docs/mycel-topics.md](docs/mycel-topics.md) |
| Crash diagnostics | [docs/faq.md](docs/faq.md#crash-diagnostics) |
| Power Pack tools (13 tools, `scripts/install.ps1` bundles them) | [docs/power-pack-tools.md](docs/power-pack-tools.md) |
