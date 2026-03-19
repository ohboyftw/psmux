---
name: add-tmux-command
description: Add new tmux-compatible commands to psmux. Use when the user wants to implement a new command like "split-window", "send-keys", "select-pane", or any tmux subcommand, or when they say "add command", "implement command", "new subcommand", "tmux compatibility", or reference a missing tmux feature they want to add.
---

# Adding a New tmux-Compatible Command to psmux

## Step-by-Step Process

### 1. Research the tmux Command
Before implementing, understand the real tmux behavior:
- Check `man tmux` or https://man7.org/linux/man-pages/man1/tmux.1.html for the canonical spec
- Note all flags, arguments, and edge cases
- Identify which flags psmux should support (start with the most common ones)

### 2. Find the Command Dispatch Point
Look in `src/` for the command dispatch/routing logic — typically a match statement or command map that routes CLI subcommands to handler functions. The pattern will look something like:

```rust
match command.as_str() {
    "new-session" => handle_new_session(args),
    "split-window" => handle_split_window(args),
    // ... add your new command here
    _ => eprintln!("Unknown command: {}", command),
}
```

### 3. Implement the Command Handler
Create the handler function following existing patterns:

```rust
fn handle_your_command(args: &[String]) -> Result<()> {
    // 1. Parse flags/arguments (follow existing arg parsing pattern)
    // 2. Connect to session via IPC (if needed)
    // 3. Execute the action
    // 4. Return result
}
```

### 4. Argument Parsing
psmux uses tmux-style argument parsing. Follow the existing pattern for flags:
- Single-letter flags: `-v`, `-h`, `-t`, `-s`
- Flags with values: `-t target`, `-s session-name`
- Positional args for commands like `send-keys`
- Special key names: `Enter`, `Tab`, `Escape`, `Space`, `C-a` through `C-z`

### 5. IPC Communication
Commands that interact with a running session need to communicate via the IPC mechanism (named pipes on Windows). Follow the existing pattern for serializing commands and sending them to the session server.

### 6. Add Tests
Write tests for:
- **Argument parsing**: valid flags, missing required args, unknown flags
- **Command execution**: expected behavior, edge cases
- **Error handling**: graceful failure with helpful messages

### 7. Update Documentation
- Add the command to `README.md` in the appropriate section
- Add to `--help` output
- Add keybinding if the command can be triggered via prefix+key

### 8. Checklist
- [ ] Command follows tmux syntax exactly (same flags, same semantics)
- [ ] Handler function has doc comments
- [ ] Error messages are user-friendly
- [ ] Tests cover happy path and edge cases
- [ ] `cargo fmt && cargo clippy -- -D warnings && cargo test` passes
- [ ] README updated
- [ ] `--help` output updated

## Currently Implemented Commands (from README)
- `new-session`, `ls`/`list-sessions`, `attach`, `has-session`, `rename-session`, `kill-session`
- `new-window`, `select-window`, `next-window`, `previous-window`, `last-window`, `kill-window`, `list-windows`
- `split-window`, `select-pane`, `kill-pane`, `resize-pane`, `swap-pane`, `rotate-window`, `zoom-pane`, `respawn-pane`, `list-panes`
- `send-keys`, `capture-pane`, `display-message`
- `set-buffer`, `paste-buffer`, `list-buffers`, `show-buffer`, `delete-buffer`

## Commonly Requested tmux Commands Not Yet Implemented
Check the codebase to see which of these are missing, then prioritize:
- `move-window`, `link-window`, `unlink-window`
- `break-pane` (promote pane to window)
- `join-pane` (move pane into another window)
- `pipe-pane` (pipe output to a command)
- `if-shell`, `run-shell`
- `set-option`, `show-options`
- `bind-key`, `unbind-key`
- `source-file` (reload config)
