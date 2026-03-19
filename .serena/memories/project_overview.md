# psmux — Project Overview

**Purpose**: Windows-native terminal multiplexer (tmux alternative) built in Rust. Provides tmux-compatible commands and keybindings for Windows Terminal, PowerShell, and cmd.exe. Binary aliases: `psmux`, `pmux`, `tmux`.

**Version**: 3.2.0 (as of March 2026)
**License**: MIT
**Repository**: https://github.com/psmux/psmux
**Platform**: Windows 10/11 only (Win32 Console APIs)

## Tech Stack
- **Language**: Rust (stable toolchain, edition 2021)
- **Key dependencies**: ratatui, crossterm, portable-pty (forked), vt100 (forked), serde/serde_json, regex, windows-sys
- **Forked crates**: `portable-pty-psmux` and `vt100-psmux` in `crates/` directory
- **Build**: cargo (release profile: LTO, single codegen unit, stripped symbols)
- **Packaging**: Chocolatey, Scoop, crates.io, GitHub Releases

## Architecture
- Session management via TCP IPC on localhost (port files in `$HOME\.psmux\`)
- Authentication via 128-bit random session keys
- Pane rendering uses Windows Console Virtual Terminal Sequences (VT100)
- Key prefix system mirrors tmux (Ctrl+b default, configurable)
- Copy mode with vim-like keybindings (1000-line scrollback default)
- Warm session claiming (v3.2.0) for instant (~50ms) session creation
- 46/47 core tmux commands implemented

## Codebase Structure
```
src/main.rs          — CLI entry, session/server logic, warm claiming
src/server/mod.rs    — Main event loop, command handlers
src/server/connection.rs — TCP handler, AUTH, command routing
src/server/helpers.rs    — Server utility functions
src/server/options.rs    — Set/get option handling
src/session.rs       — Port/key files, warm session detection
src/pane.rs          — PTY spawning, ConPTY, warm pane pre-spawn
src/client.rs        — Remote rendering, frame serialization
src/commands.rs      — Action parsing, command-to-action mapping
src/format.rs        — Format string expansion (#{...})
src/platform.rs      — Windows Console API, mouse, VT processing
src/types.rs         — Data structures: Pane, Window, CtrlReq
src/cli.rs           — CLI argument parsing, target resolution
src/config.rs        — Configuration file parsing
src/input.rs         — Keyboard input handling
src/layout.rs        — Pane layout management
src/rendering.rs     — Screen rendering
src/copy_mode.rs     — Copy mode (vim-like)
src/style.rs         — Style/color handling
src/app.rs           — Application state
src/tree.rs          — Tree data structure
src/util.rs          — Utility functions
src/help.rs          — Help text
src/debug_log.rs     — Debug logging
src/ssh_input.rs     — SSH input handling
src/window_ops.rs    — Window operations
crates/              — Forked portable-pty and vt100
tests/               — PowerShell integration tests
packages/chocolatey/ — Chocolatey packaging
scripts/             — Install/uninstall PowerShell scripts
```

## Swarm Backend
psmux serves as the tmux spawn backend for Claude Code's TeammateTool on Windows. All 12 critical tmux commands for swarm orchestration are implemented and validated.
