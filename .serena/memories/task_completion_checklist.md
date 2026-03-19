# Task Completion Checklist

When a coding task is completed, run the following before submitting:

1. `cargo fmt` — Auto-format all code
2. `cargo clippy -- -D warnings` — Lint with warnings as errors
3. `cargo test` — Run all unit and integration tests
4. If swarm-related changes: `pwsh tests/validate-swarm-backend.ps1`
5. Verify all new public functions have `///` doc comments
6. Verify all new `unsafe` blocks have `// SAFETY:` comments
7. Conventional commit message format
