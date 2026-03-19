---
description: Run the full pre-PR check suite (format, lint, test)
---

Run the complete psmux pre-PR checklist:

1. Run `cargo fmt --check` and report any formatting issues
2. Run `cargo clippy -- -D warnings` and report any lint warnings
3. Run `cargo test` and report any test failures
4. If any step fails, explain the issue and suggest a fix
5. If all pass, confirm the code is ready for PR

Format the output as a checklist with pass/fail status for each step.
