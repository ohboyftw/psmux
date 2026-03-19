---
description: Review the current diff for psmux code quality and correctness
---

Review the current staged or unstaged git diff. Check for:

1. **Rust idioms**: proper use of Result/Option, no unwrap in non-test code, iterator patterns preferred over manual loops
2. **Windows API safety**: all unsafe blocks have SAFETY comments, return values checked, handles properly closed
3. **tmux compatibility**: new commands match real tmux behavior and flag conventions
4. **Error messages**: user-friendly, actionable error messages (not raw Win32 error codes)
5. **Tests**: new functionality has corresponding tests
6. **Documentation**: public functions have doc comments, README updated if user-facing
7. **No regressions**: `cargo test` still passes

Provide feedback organized by severity: 🔴 Must fix, 🟡 Should fix, 🟢 Nice to have.
