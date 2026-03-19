# Code Style & Conventions

## Rust Conventions
- All public functions must have doc comments (`///`)
- Unsafe blocks require a `// SAFETY:` comment explaining the invariant
- Error handling: `anyhow::Result` for binary, `thiserror` for library errors
- Windows API calls use the `windows-sys` crate; wrap raw FFI in safe abstractions
- Unit tests in `#[cfg(test)]` modules alongside source
- Integration tests in `tests/` directory (PowerShell scripts)

## Naming
- Standard Rust naming: snake_case for functions/variables, PascalCase for types/enums
- tmux-compatible command names use kebab-case (e.g., `new-session`, `split-window`)
- Enum variants for commands use PascalCase (e.g., `CtrlReq::SendKeys`)

## Code Organization
- New tmux-compatible commands follow the existing command dispatch pattern in `src/`
- Command routing: CLI (main.rs) → TCP connection (server/connection.rs) → Server event loop (server/mod.rs)
- Format strings (`#{...}`) handled in `src/format.rs`
- Target resolution (`session:window.pane`) in `src/cli.rs`

## Testing
- Unit tests: `#[cfg(test)]` modules in source files
- Integration tests: PowerShell scripts in `tests/` directory
- Swarm validation: `tests/validate-swarm-backend.ps1`

## CI Requirements
- Must pass: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
