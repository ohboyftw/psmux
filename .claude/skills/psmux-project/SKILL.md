---
name: psmux-project
description: >
  Core psmux development knowledge — Rust conventions, testing strategy, release
  workflow, Windows API patterns, tmux command implementation, and unsafe code
  auditing. Use when working on psmux source code, adding features, fixing bugs,
  reviewing PRs, writing tests, building releases, or debugging Windows-specific
  issues. Also triggers for "cargo build", "cargo test", "clippy", "unsafe",
  "Win32", "ConPTY", "VT100", "named pipe", "pane rendering", or any discussion
  of psmux internals.
---

# psmux Project Development Guide

## Building & Testing

```bash
cargo build                              # debug build
cargo build --release                    # optimized build
cargo test                               # all tests
cargo test -- --nocapture                # with stdout
cargo test --lib                         # unit tests only
cargo test --test '*'                    # integration tests only
cargo clippy -- -D warnings             # lint
cargo fmt --check                        # format check
```

Always run `cargo fmt && cargo clippy -- -D warnings && cargo test` before committing.

## Adding a New tmux Command

1. Look up real tmux behavior at https://man7.org/linux/man-pages/man1/tmux.1.html
2. Find the command dispatch point in `src/` (match statement or command map)
3. Add the new command to the dispatch table
4. Implement handler following existing patterns:
   - Parse flags (tmux-style: `-v`, `-h`, `-t target`, `-s name`)
   - Connect to session via IPC if needed
   - Execute the action
   - Return result
5. Add unit tests for arg parsing + behavior
6. Add integration test in `tests/`
7. Update `--help` output
8. Update README.md in the correct section
9. Run full CI check

## Testing Patterns

Unit tests: inside source files in `#[cfg(test)] mod tests { ... }`
Integration tests: in `tests/` directory, each `.rs` is a separate binary.
Windows-specific tests: gate with `#[cfg(target_os = "windows")]`.

Key areas to cover: command parsing, keybinding resolution, config parsing,
pane geometry (split calculations, resize, boundaries), session lifecycle,
format variable expansion, copy mode state, buffer management.

## Unsafe Code Rules

Every `unsafe` block MUST have a `// SAFETY:` comment. Scope should be minimal —
only the FFI call itself, not surrounding logic. Return values must be checked
(`BOOL` 0 = failure, `HANDLE` null/INVALID = failure). Handles must be closed
via RAII (struct with Drop impl calling CloseHandle). Buffer sizes must match
the actual allocation. Thread safety must be considered for shared console handles.

Audit command: `grep -rn "unsafe" src/ --include="*.rs"`
Missing safety comments: `grep -B1 "unsafe {" src/ --include="*.rs" | grep -v "SAFETY"`

## Windows API Surface

- Console rendering: Virtual Terminal Sequences (VT100), `SetConsoleMode` with
  `ENABLE_VIRTUAL_TERMINAL_PROCESSING`
- Input: `ReadConsoleInput` → `KEY_EVENT_RECORD`, `MOUSE_EVENT_RECORD`
- Pseudo-console: `CreatePseudoConsole` (Windows 10 1809+)
- IPC: Named pipes (`\\.\pipe\psmux-session-name`)
- Process spawning: `CreateProcessW` with `STARTUPINFOW`
- Screen buffer: `GetConsoleScreenBufferInfo`, `WINDOW_BUFFER_SIZE_EVENT`

## Release Workflow

1. Update version in `Cargo.toml` + `packages/chocolatey/*.nuspec`
2. Update CHANGELOG.md (Keep a Changelog format)
3. Commit: `chore: bump version to vX.Y.Z`
4. Tag: `git tag vX.Y.Z && git push origin vX.Y.Z`
5. Build: `cargo build --release`
6. Publish: `cargo publish` (crates.io), `choco push` (Chocolatey)
7. Verify install script works
