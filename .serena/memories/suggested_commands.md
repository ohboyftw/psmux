# Suggested Commands

## Build & Run
- `cargo build` — Debug build
- `cargo build --release` — Optimized release build
- `cargo run` — Run psmux (debug)
- `cargo run -- new-session -s work` — Run with arguments

## Quality Checks (run before PR)
- `cargo fmt --check` — Check formatting
- `cargo fmt` — Auto-format
- `cargo clippy -- -D warnings` — Lint (warnings as errors)
- `cargo test` — Run all tests
- Full pre-PR: `cargo fmt && cargo clippy -- -D warnings && cargo test`

## Testing
- `cargo test` — All tests
- `cargo test -- module_name` — Single module
- `pwsh tests/validate-swarm-backend.ps1` — Swarm backend validation

## Packaging
- `cd packages/chocolatey && choco pack` — Chocolatey package
- `pwsh scripts/publish-choco.ps1` — Publish to Chocolatey

## System Commands (Windows)
- `git` — Version control
- `ls` / `dir` — List files (PowerShell)
- `cd` — Change directory
- `grep` / `Select-String` — Search content
- `find` / `Get-ChildItem -Recurse` — Find files
- `pwsh` — PowerShell 7+
