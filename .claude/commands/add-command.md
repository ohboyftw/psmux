---
description: Scaffold a new tmux-compatible command
allowed-tools: Read, Write, Edit, Bash
---

Add a new tmux-compatible command to psmux: $ARGUMENTS

Steps:
1. Look up the real tmux behavior for this command (flags, arguments, semantics)
2. Find the command dispatch/routing point in the source code
3. Add the new command to the dispatch table
4. Create the handler function with:
   - Proper argument parsing (matching tmux flag conventions)
   - Doc comments explaining the command
   - Error handling with user-friendly messages
5. Add unit tests for argument parsing and core behavior
6. Update the `--help` text to include the new command
7. Add the command to README.md in the appropriate section
8. Run `cargo fmt && cargo clippy -- -D warnings && cargo test` to verify

Follow the existing code patterns in the project for consistency.
