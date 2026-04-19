# Scripting & Automation

psmux supports tmux-compatible commands for scripting and automation.

## Window & Pane Control

```powershell
# Create a new window
psmux new-window

# Split panes
psmux split-window -v          # Split vertically (top/bottom)
psmux split-window -h          # Split horizontally (side by side)

# Navigate panes
psmux select-pane -U           # Select pane above
psmux select-pane -D           # Select pane below
psmux select-pane -L           # Select pane to the left
psmux select-pane -R           # Select pane to the right

# Navigate windows
psmux select-window -t 1       # Select window by index (default base-index is 1)
psmux next-window              # Go to next window
psmux previous-window          # Go to previous window
psmux last-window              # Go to last active window

# Kill panes and windows
psmux kill-pane
psmux kill-window
psmux kill-session
```

## Sending Keys

```powershell
# Send text directly
psmux send-keys "ls -la" Enter

# Send keys literally (no parsing)
psmux send-keys -l "literal text"

# Special keys supported:
# Enter, Tab, Escape, Space, Backspace
# Up, Down, Left, Right, Home, End
# PageUp, PageDown, Delete, Insert
# F1-F12, C-a through C-z (Ctrl+key)
```

## Pane Information

```powershell
# List all panes in current window
psmux list-panes

# List all windows
psmux list-windows

# Capture pane content
psmux capture-pane

# Display formatted message with variables
psmux display-message "#S:#I:#W"   # Session:Window Index:Window Name
```

### `capture-pane` negative `-S`/`-E` row offsets

`-S` (start row) and `-E` (end row) accept negative values as offsets from
the bottom of the scrollback buffer. Negative values that would point before
row 0 are silently clamped to row 0 — the command never errors on out-of-range
offsets.

```powershell
# Tail the last 20 lines of scrollback (clamps if buffer is shorter)
psmux capture-pane -t %3 -S -20 -p

# Capture just the bottom line (single row at -E -1 clamps start to row 0
# when scrollback is short, so at most one row is returned)
psmux capture-pane -t %3 -S -5 -E -1 -p
```

If the buffer contains fewer lines than the requested window, the output is
simply shorter than requested. A wildly out-of-range `-S` (e.g. `-S -10000`)
is accepted and returns the full visible pane content with exit code 0.

## Running a command in a pane (`psmux exec`)

`psmux exec` runs a command directly in a target pane and returns the PID
and exit code. Useful for agents that need to drive a specific pane and
block until the command finishes without parsing screen output.

```powershell
# Run a command in pane %3, block until it exits
psmux exec -t %3 -- pytest -k fast

# Detached: spawn, print PID, exit immediately
psmux exec -t %3 -d -- long-running-task

# Sibling verbs: new-window / split-window also accept -- command
psmux new-window -- cargo test
psmux split-window -- bash -c "tail -f /var/log/app.log"
```

Same wire path is exposed over JSON-RPC on `CustomPaneBackend` as the
`exec` method. Push events: `context_ready` fires when the pane reaches
an idle prompt, `exec_completed` fires when the command finishes
(payload includes `exit_code` and `elapsed_ms`), `context_exited` fires
when the pane itself exits (adds `elapsed_ms` + `command`).

## Spawning with a raw argv (`new-window --raw`)

By default, `new-window -- <cmd>` wraps `<cmd>` in the user's default
shell (`pwsh -Command`, `bash -c`, or `cmd /C`). Shell operators like
`>`, `|`, `&&` are interpreted by the wrapper shell before reaching the
inner command — on Windows, pwsh intercepts `>` as its own pipeline
redirect, which can silently break `cmd /c echo X > path` style
invocations.

`--raw` bypasses the wrapper and spawns argv[0] directly with argv[1..]
as arguments:

```powershell
# Wrapped (default): pwsh -NoLogo -Command "cmd /c echo A > path"
psmux new-window -- cmd /c "echo A > path"

# Raw: spawns cmd.exe with /c and the rest of argv as-is
psmux new-window --raw -- cmd /c "echo A > path"
```

Orchestrate workers (see `docs/orchestrate.md`) always use `--raw` — a
plan's `command: ["cmd","/c","echo A > path"]` is guaranteed to behave
the same whether the caller's login shell is pwsh, bash, or cmd.

## Waiting for command completion

Don't poll with `sleep N; capture-pane` in a loop. psmux has server-side
`wait-for` that tails the live VT100 screen — zero shell roundtrips, 50ms
internal poll. Replaces the tmux-community "marker + sleep" pattern.

```powershell
# Send a command and wait for it to finish (marker pattern, server-side)
psmux send-keys -t mypane "pytest -k fast; echo __DONE__" Enter
psmux wait-for -t mypane --output "__DONE__" --timeout 30000

# Then capture the result cleanly
psmux capture-pane -t mypane -p | Select-String -NotMatch "__DONE__"
```

Other `wait-for` modes:

```powershell
psmux wait-for -t mypane --ready            # pane idle ≥500ms (prompt returned)
psmux wait-for --exit 12345 --timeout 60000 # process PID exits
psmux wait-for --file out.log --timeout 30000 # filesystem appears
```

All `--timeout` values are **milliseconds**. Exit codes: `0` success,
`1` timeout (condition never met within the window), `2` error (bad
target, invalid regex, etc.). With `--json`, the server emits a
`WaitOutcome` object and the mapping is exact: `{kind:"success"}` or
`{kind:"exit_success"}` → 0; `{kind:"timeout"}` → 1; anything else → 2.
Without `--json`, the server writes a `TIMEOUT` or `ERROR: …` line
which the CLI maps the same way. Add `--json` for machine-readable
`WaitOutcome` (`{kind: success|exit_success|timeout|error, ...}`).

## Blocking on a pane's child process (`wait-pane`)

`wait-pane` blocks until a target pane's child process exits, returning
the process's exit code as the CLI's own exit code. Unlike `wait-for
--exit PID`, this tracks the pane's *current* child (survives
respawn-pane `-k`) rather than a specific OS PID.

```powershell
# Block until pane %3's child exits; returns its exit code
psmux wait-pane -t %3

# With timeout (milliseconds)
psmux wait-pane -t %3 --timeout 30000

# Wait for the pane to reach an idle prompt (not for child exit)
psmux wait-pane -t %3 --ready --timeout 10000
```

Use `wait-pane` when you spawned a command via `new-window --`,
`split-window --`, or `exec` and want to block the CLI until it
finishes. Use `wait-for --output REGEX` when you need to detect an
arbitrary marker in the screen buffer without the child exiting.

For agents using JSON-RPC, `CustomPaneBackend.wait_for` exposes the same
four modes. For simple "fire-and-forget-with-result", `psmux run -- cmd`
atomically combines send + wait + capture, returning exit code.

Stale-marker caveat: if the same script runs against multiple panes,
generate a per-invocation marker (e.g. `__DONE_$([guid]::NewGuid().Guid.Substring(0,8))__`)
rather than hardcoding `__DONE__` — otherwise a parallel pane's earlier
output could short-circuit the wait.

## Paste Buffers

```powershell
# Set paste buffer content
psmux set-buffer "text to paste"

# Paste buffer to active pane
psmux paste-buffer

# List all buffers
psmux list-buffers

# Show buffer content
psmux show-buffer

# Delete buffer
psmux delete-buffer
```

## Pane Layout

```powershell
# Resize panes
psmux resize-pane -U 5         # Resize up by 5
psmux resize-pane -D 5         # Resize down by 5
psmux resize-pane -L 10        # Resize left by 10
psmux resize-pane -R 10        # Resize right by 10

# Swap panes
psmux swap-pane -U             # Swap with pane above
psmux swap-pane -D             # Swap with pane below

# Rotate panes in window
psmux rotate-window

# Toggle pane zoom
psmux zoom-pane
```

### Minimum pane size for splits

psmux rejects a split when the resulting pane would be too small to run a
shell. The minimum is **3 rows** for a vertical split (one row higher than
upstream tmux's 2-row floor). The extra row is needed because psmux's default
`pane-border-status=top` renders a title bar that consumes one row of each
pane's visible area, leaving at least 2 shell-content rows per pane after the
split.

For horizontal splits the minimum is **10 columns**.

When a split is rejected you receive a clear error:

```
pane too small to split vertically (6 rows, need 7)
pane too small to split horizontally (19 cols, need 21)
```

Repeated splits on the same pane will eventually trigger this guard; resize
the window or zoom out before continuing.

## Session Management

```powershell
# Check if session exists (exit code 0 = exists)
psmux has-session -t mysession

# Rename session
psmux rename-session newname

# Respawn pane — NOTE: bare respawn-pane is a NO-OP on a live pane (PID unchanged).
# The server rejects the request if the pane's child is still running.
# Use -k to force-kill the child and restart:
psmux respawn-pane -k          # kills the current shell, spawns a fresh one
psmux respawn-pane             # safe no-op on a live pane; only restarts a dead pane
```

## Environment Variables

```powershell
# Set a global env var (inherited by all new panes)
psmux set-environment -g EDITOR vim

# Set a session-scoped env var
psmux set-environment MY_VAR value

# Unset a global env var
psmux set-environment -gu MY_VAR

# Show all environment variables
psmux show-environment
psmux show-environment -g
```

## Format Variables

The `display-message` command supports these variables:

| Variable | Description |
|----------|-------------|
| `#S` | Session name |
| `#I` | Window index |
| `#W` | Window name |
| `#P` | Pane ID |
| `#T` | Pane title |
| `#H` | Hostname |
| `#{pane_id}` | Numeric pane id including `%` prefix (e.g., `%3`) |
| `#{pane_pid}` | OS PID of the pane's current child process |
| `#{pane_dead}` | `1` if the pane's child has exited, `0` otherwise |
| `#{pane_exit_code}` | Last child's exit code, empty if still alive |
| `#{pane_current_path}` | Pane's cwd (synced via `__psmux_sync_cwd` hook) |
| `#{window_id}` | Numeric window id including `@` prefix |

## Advanced Commands

```powershell
# Discover supported commands
psmux list-commands

# Server/session management
psmux kill-server
psmux list-clients
psmux switch-client -t other-session

# Config at runtime
psmux source-file ~/.psmux.conf
psmux show-options
psmux set-option -g status-left "[#S]"

# Layout/history/stream control
psmux next-layout
psmux previous-layout
psmux clear-history
psmux pipe-pane -o "cat > pane.log"

# Hooks
psmux set-hook -g after-new-window "display-message created"
psmux show-hooks
```

## Config reload

`source-file` re-reads a config file into the running server without
restarting. psmux tracks whether the session's key table was cleared by
`unbind-key -a` via an internal `defaults_suppressed` flag.

**Reload sequence and flag behavior:**

| Action | `defaults_suppressed` | Result |
|--------|----------------------|--------|
| `source-file` a file containing `unbind-key -a` | `true` | All built-in prefix bindings removed; `list-keys` returns < 5 bindings |
| `source-file` a plain config (no `unbind-key -a`) | cleared → `false` | Default bindings re-registered on top of any new `bind-key` lines |

```powershell
# Step 1: load a "clean slate" config that strips all defaults
Set-Content C:\tmp\clean.conf "unbind-key -a"
psmux source-file C:\tmp\clean.conf
# list-keys now returns almost nothing

# Step 2: reload a normal config — defaults come back
Set-Content C:\tmp\normal.conf "# standard config"
psmux source-file C:\tmp\normal.conf
# list-keys now returns 20+ default bindings
```

This means a two-file pattern works correctly:
1. A "base" config with only `unbind-key -a` plus explicit `bind-key` lines.
2. A "user" overlay loaded second that overrides without adding bare `unbind-key -a`.

The reload is per-session (not server-global); each `-t session` reload
affects only that session's key table.

### `show-options -v` value-only output

The `-v` flag switches `show-options` to value-only output — no `key value`
formatting, just the bare value string(s).

```powershell
# Named option: prints only the value, no key prefix
psmux show-options -g -v status-left
# Output: [#S]

# No option name: prints every option's value, one per line (no key prefixes)
psmux show-options -g -v
# Output (example):
# [#S]
# %H:%M %d-%b-%y
# bg=green,fg=black
# ...
```

Combine with a named option for scripts that need to extract a setting
without parsing `key value` pairs:

```powershell
$shell = (psmux show-options -g -v default-shell).Trim()
```

## Windows-specific behavior

### No console-window flash (`CREATE_NO_WINDOW`)

On Windows, background processes spawned by `run-shell`, `if-shell`, and
format `#()` expansion use the `CREATE_NO_WINDOW` flag. This prevents a
`ConsoleWindowClass` / `PseudoConsoleWindow` from briefly appearing on screen
every time a hook or conditional fires — a visible flash that plain
`subprocess::Command` calls cause on Windows when a new console is allocated.

The flag is applied transparently; no config is required. Running
`run-shell` in a tight loop (e.g. 5 invocations in 300 ms) produces zero
extra visible console windows.

```tmux
# These all run silently in the background — no window flash
set-hook -g after-new-window "run-shell 'date /t >> C:/tmp/psmux-log.txt'"
if-shell "exit 0" "display-message 'conditional-ran'"
```

## Target Syntax (`-t`)

psmux supports tmux-style targets:

```powershell
# window by index in session
psmux select-window -t work:2

# specific pane by index
psmux send-keys -t work:2.1 "echo hi" Enter

# pane by pane id
psmux send-keys -t %3 "pwd" Enter

# window by window id
psmux select-window -t @4
```
