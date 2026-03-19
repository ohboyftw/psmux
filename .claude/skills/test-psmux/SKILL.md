---
name: test-psmux
description: Run and write tests for psmux. Use when the user asks to "test", "add tests", "write tests", "fix failing tests", "run tests", "coverage", "integration test", or discusses test strategy for terminal multiplexer functionality. Also use when debugging test failures or CI red builds.
---

# Testing psmux

## Running Tests

```bash
# All tests
cargo test

# Specific test
cargo test test_name

# Specific module
cargo test -- module_name

# With output (for debugging)
cargo test -- --nocapture

# Only integration tests
cargo test --test '*'

# Only unit tests (lib)
cargo test --lib
```

## Test Organization

- **Unit tests**: Inside each source file in `#[cfg(test)] mod tests { ... }`
- **Integration tests**: In `tests/` directory — each `.rs` file is a separate test binary

## Writing Tests for psmux

### Unit Test Template
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptive_name() {
        // Arrange
        let input = ...;

        // Act
        let result = function_under_test(input);

        // Assert
        assert_eq!(result, expected);
    }
}
```

### Key Areas to Test
1. **Command parsing** — tmux command syntax (`new-session -s name`, `split-window -v`, `send-keys "text" Enter`)
2. **Key binding resolution** — prefix + key → action mapping
3. **Configuration parsing** — `.psmux.conf` file parsing, `set -g` syntax
4. **Pane geometry** — split calculations, resize operations, boundary conditions
5. **Session management** — create, attach, detach, list, kill
6. **Format variables** — `#S`, `#I`, `#W`, `#P`, `#T`, `#H` expansion
7. **Copy mode** — selection state, yank, scroll position
8. **Buffer management** — set-buffer, paste-buffer, list-buffers

### Testing Windows-Specific Code
Since psmux is Windows-only, tests that call Win32 APIs should be gated:
```rust
#[test]
#[cfg(target_os = "windows")]
fn test_windows_specific_feature() { ... }
```

For cross-platform CI (if any), mock the Windows APIs or use `#[cfg(not(target_os = "windows"))]` stubs.

### Integration Test Template
Create `tests/test_feature.rs`:
```rust
use std::process::Command;

#[test]
fn test_cli_help_output() {
    let output = Command::new("cargo")
        .args(["run", "--", "--help"])
        .output()
        .expect("Failed to execute");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("psmux"));
    assert!(output.status.success());
}
```

## Before Submitting
Always run the full check:
```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```
