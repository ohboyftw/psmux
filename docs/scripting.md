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

`kill-session` exits with code 1 and prints "no server running on session
'<name>'" when it reaches no server, as tmux does. Before this it exited 0 even
when nothing was killed, so a cleanup step that kills a session that may already
be gone should not treat a non-zero exit as a failure.

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

### Waiting for a pane to settle before typing (`send-keys --wait-ready`)

A freshly spawned shell drops keys that arrive before it has drawn its
prompt. `--wait-ready` makes the server hold the keys until the target pane
is ready, then deliver them:

```powershell
$pane = psmux new-window -d -P -F '#{pane_id}'
psmux send-keys -t $pane --wait-ready "cargo build" Enter
```

"Ready" is the same test as `#{pane_ready}` and `wait-pane --ready`, and it
means idle. If the pane's program reports its state with OSC 7501 (see
`#{pane_agent_state}`) as `idle`, `working` or `blocked`, that reported state
decides: `idle` is ready, and a pane reporting `working` or `blocked` is not
ready even when its output is silent. Otherwise, including when only `done` or
`error` records are left, ready means the pane has produced output and then
been silent for 500 ms; that is not prompt detection.
`#{pane_agent_readiness}` shows the state the rule reached and
`#{pane_agent_source}` tells you which rule applied.

The wait happens on the server, not in the command. `send-keys` hands the
request to the server and returns at once with exit code 0; it does not wait
for the keys to go out and never sees the result. The server checks every
100 ms for up to 30 seconds; if the pane never becomes ready, it sends the
keys anyway. That includes an agent waiting at a `blocked` prompt: keys meant
to answer it arrive after the 30 seconds, so send those without
`--wait-ready`.

Name the pane by id, and always qualify it: write `-t sess:.%N`. A bare
`-t %N` is not searched for across sessions. `send-keys` sends it to your own
session when you run it inside a psmux pane, and otherwise to the most
recently used session. If that session has no such pane, the server answers
`can't find pane` but `send-keys` has already exited 0, so the keys go
nowhere without an error. Without `-t`, or with a pane index such as `-t :0.1` or
`-t sess:0.1`, there is no wait and the keys go out at once. The keys go to
the `-t` pane, not to the pane that is active when they are sent.

If the pane closes while the server waits, the server drops the keys and
sends them nowhere, but `send-keys` has already exited 0, so a script that
checks `$LASTEXITCODE` cannot see this. To make sure the pane is still there,
check it before you send, with `wait-pane --ready` or `#{pane_ready}`.

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
psmux wait-for -t mypane --ready            # pane idle (#{pane_ready})
psmux wait-for -t %3 --until blocked --timeout 600000 # agent state, see below
psmux wait-for --exit 12345 --timeout 60000 # process PID exits
psmux wait-for --file out.log --timeout 30000 # filesystem appears
```

All `--timeout` values are **milliseconds**. Exit codes: `0` success,
`1` timeout (condition never met within the window), `2` error (bad
target, invalid regex, the server stopping during the wait, etc.). With `--json`, the server emits a
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

# Wait for the pane to be idle, as #{pane_ready} (not for child exit)
psmux wait-pane -t %3 --ready --timeout 10000

# Wait for the pane's agent to report a state (OSC 7501)
psmux wait-pane -t %3 --until blocked --timeout 600000
```

### Waiting for an agent state (`--until`)

`wait-pane -t %N --until STATE` and `wait-for -t %N --until STATE` wait
for the pane's agent state, where STATE is one of `idle`, `working`,
`blocked`, `done` or `error`. Exit codes: `0` the state was seen, `1`
the `--timeout` passed first, `2` the pane is dead or gone, including a
target that no longer exists when the command runs and a server that
stops during the wait (or STATE is not one of the five, `--until` is
combined with another wait condition, or `wait-pane` has no `-t`).
`wait-for --json` gives `{kind:"error",reason:"pane is dead or gone"}`
for a dead or gone pane, whether it was gone before the call or died
during the wait, and `{kind:"error",reason:"server closed the connection"}`
when the server stopped before it could answer.

- `--until idle` is `--ready`: the same rule as `#{pane_ready}`, a reported
  `idle` or 500 ms of output silence. `--until` cannot be combined with
  `--ready`, `--file`, `--exit` or `--output`. One difference: on a dead pane `--until idle` returns `2`,
  while `--ready` still judges the dead pane by its silence.
- `--until working|blocked|done|error` waits for a **reported** state: the
  pane's worst-of OSC 7501 record, as `#{pane_agent_state}` shows it. Output
  alone never counts as `working`, so a pane whose program does not report
  never satisfies these, and a command that prints and exits returns `2`.
  A `blocked` background task (a child record) satisfies `--until blocked`
  even while the main session is `working`.
- The wait is level-triggered: a pane already in STATE returns `0` at once.
  It is meant for states that persist. The server checks once per 100 ms
  tick, so a state held for less than that can be missed.
- A dead pane can only satisfy `done` or `error`, the two states that
  survive process exit, and only while `remain-on-exit` keeps the pane.
  Without it, `done`/`error` vanish with the pane and the wait returns `2`.
- `wait-pane --until` without `--timeout` blocks until the state is seen
  or the pane dies, like `wait-pane --ready`. `wait-for` defaults to a
  one-hour timeout. If the CLI is killed during such a wait, the server
  keeps the waiter queued until the pane dies or reaches the state, so
  give long waits a `--timeout`.
- Name the pane by id (`%N`, or `sess:.%N` for another session). `wait-for`
  without an id target watches the active pane. `wait-pane --until` needs
  an id: a `window.pane` index target exits `2`, not `1`.

Use `wait-pane` when you spawned a command via `new-window --`,
`split-window --`, or `exec` and want to block the CLI until it
finishes. Use `wait-for --output REGEX` when you need to detect an
arbitrary marker in the screen buffer without the child exiting.

For agents using JSON-RPC, `CustomPaneBackend.wait_for` exposes the same
five modes (`state` is `--until`). For simple "fire-and-forget-with-result", `psmux run -- cmd`
atomically combines send + wait + capture, returning exit code.

Stale-marker caveat: if the same script runs against multiple panes,
generate a per-invocation marker (e.g. `__DONE_$([guid]::NewGuid().Guid.Substring(0,8))__`)
rather than hardcoding `__DONE__` — otherwise a parallel pane's earlier
output could short-circuit the wait.

## Agent state toasts

When a pane's agent reports a new OSC 7501 state, psmux shows a Windows
toast if the pane's worst-of state (the one `#{pane_agent_state}` shows)
becomes `blocked`, `error` or `done`. A `blocked` background task of a
Claude Code session toasts even while the main session is `working`.
Turn this off with `set -g agent-toasts off` (default `on`).

- The heading names the app and the reason, for example
  `claude-code needs permission`, `claude-code has a question`,
  `Agent reported an error`. The text is `session %N: msg`, with the
  record's `title` when it has no `msg`. Both are cleaned the same way as
  `#{pane_agent_msg}` and are passed to PowerShell as environment
  variables, never as part of a command.
- `working` and `idle` never toast, and a pane whose program does not
  report OSC 7501 never toasts, however much output it prints.
- One pane does not toast `done` or `error` twice in a row within 30 s,
  and waits at least 5 s between any two of its toasts. `blocked` has no
  30 s window: every new prompt is a new question, so a pane that is
  asked, answered and asked again toasts twice, the second time once its
  5 s gap has passed. Agent toasts as a whole are at least 1 s apart, and
  only one toast (agent or OSC 99/777) is shown at a time. A due agent
  toast is shown before the OSC 99/777 notifications of the same tick. A
  toast that has to wait is kept, not dropped: each pane keeps its most
  important waiting toast (blocked > error > done).
- Toasts follow the worst-of state, so they fire only when it changes.
  While a pane is already `blocked`, a further blocked record (another
  background task, or a new `kind` or `msg`) does not toast.
- A waiting `blocked` toast is dropped if the pane leaves `blocked` before
  it is shown: the question was already answered. A waiting toast is also
  dropped when its pane is killed (`kill-pane`, `kill-window`). A waiting
  `done` or `error` toast is still shown after the pane's program exits on
  its own.
- The server samples the state once per 100 ms, so a state held for less
  than that can be missed. A pane's final state at exit is read when the
  pane is reaped, so `done` followed by exit still toasts.
- When the last pane exits and the server shuts down (`exit-empty`, the
  default), the most important waiting toast is started just before the
  server exits, ignoring the limits above. A toast that was already being
  started in that same moment may not appear.

## Resuming agents after a restart (`resurrect`)

psmux saves a snapshot of each session's layout, and `psmux resurrect <name>`
rebuilds the session from it. When a pane was running an agent that reports
its state with OSC 7501, the snapshot records the agent's `app` name, and
resurrect starts the agent's resume command in that pane instead of the
pane's original command:

| `app` | Command run on resurrect |
|-------|--------------------------|
| `claude-code` | `claude --continue` |
| `pi` | `pi --continue` |

- The command comes only from this table. The `app` name is reported by the
  pane, so any program in the pane can pick a row, but it can never supply the
  command. Other `app` names are not recorded.
- `--continue` resumes the most recent conversation of the current directory.
  psmux therefore restores the pane in the agent's own working directory, read
  from the agent process when the snapshot is written: the first process below
  the pane's shell that is not itself a shell. Deeper processes, such as MCP
  servers or commands the agent runs, are not used, since they can run in
  another directory.
- `app` is recorded only while the agent process is still running in a live
  pane. A done or error record that survives the agent's exit does not make
  the pane resume, and neither does a pane kept by `remain-on-exit`.
- Two agent panes in the same directory both resume that directory's most
  recent conversation. Options given when the agent was started (`--model`,
  teammate flags) are not carried over. Neither Claude Code nor Pi tells the
  terminal its session id, so a resume by id is not possible.
- Only agents running interactively report OSC 7501 (`pi --print` workers do
  not), so only those are resumed.
- The snapshot is written when an agent appears in a pane or its last record
  goes away (for example Claude Code sends `state=clear` when it exits), and,
  for a session served by a background server, synchronously on
  `kill-server`. Changes between states never write it.
- The resume command runs through the pane's default shell in the agent's
  directory. When that shell is `cmd.exe`, which looks in the current
  directory before `PATH`, a `claude.bat` or `pi.cmd` placed in that
  directory runs instead of the real agent. PowerShell does not search the
  current directory.
- When the resumed agent exits, the pane exits with it; it does not fall back
  to the shell that originally ran the agent.

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
| `#{pane_agent}` | The pane's `@agent` option (`set-option -p @agent ...`), not the reported agent state |
| `#{pane_agent_state}` | State the pane's program reports with OSC 7501: `idle`, `working`, `done`, `blocked` or `error`; empty when nothing is reported or after `state=clear` |
| `#{pane_agent_kind}` | Why the worst record is blocked: `permission`, `question` or `auth`; empty otherwise |
| `#{pane_agent_progress}` | Progress 0-100 of the worst record; empty when not reported |
| `#{pane_agent_app}` | App name of the worst record, inherited from its parent record (Claude Code reports `claude-code`) |
| `#{pane_agent_title}` | Title of the worst record |
| `#{pane_agent_msg}` | One-line message of the worst record |
| `#{pane_agent_readiness}` | State the `#{pane_ready}` rule reached: the reported `idle`, `working` or `blocked`, or, by output silence, `idle` (silent for 500 ms) or `working`; empty when nothing is reported and the pane has produced no output yet |
| `#{pane_agent_source}` | Which rule `#{pane_ready}` used: `reported` when the pane has an `idle`, `working` or `blocked` OSC 7501 record, `output_stable` otherwise. `output_stable` with an empty `#{pane_agent_readiness}` means no output yet, not silence |
| `#{pane_ready}` | `1` when `#{pane_agent_readiness}` is `idle` |
| `#{window_agent_state}` | Worst `#{pane_agent_state}` over the window's panes, in the same order; empty when no pane reports |

`#{pane_agent_state}` is the worst state over all of the pane's OSC 7501
records, in the order blocked, error, working, done, idle. A Claude Code
background task that is `blocked` therefore shows as `blocked` even while the
main session is `working`. The server samples the state once per 100 ms tick,
so a state held for less than one tick (a quick `done` then `idle`, or a short
`blocked`) can be missed. `clear-history` and `respawn-pane` start the pane
with a fresh terminal, so the value is empty until the program reports again.

The choose-tree overlay (`prefix + w`) shows the same state
as a glyph at the start of each row: `!` blocked, `✗` error, `●` working, `✓`
done and `○` idle, with a blank when nothing is reported. The glyph comes first
so a long pane title that runs past the overlay's edge never hides it. A window
row shows the worst state of its panes and a session header the worst of its
windows, so a blocked pane in a collapsed session still marks that session.
While the `prefix + w` overlay is open it asks every session for its tree again
about once a second, all sessions at once, so a glyph changes within about a
second plus the time the slowest session takes to answer (at most 150 ms) of
the server's 100 ms sample. The cursor stays on the same row when the list
changes. A session that misses a refresh keeps its rows; it is dropped after
three missed refreshes in a row, or at once when its server is gone. If no
session answers when the overlay opens, it lists the current session's windows
and panes from the last layout the client received, without glyphs, because
those states may be old. A server older than this feature reports no state,
and its rows show no glyph.

The glyphs need a font that has them. `●` and `○` are East Asian Ambiguous
width, so under a CJK code page, or a font that draws them two cells wide, the
overlay's right border can shift by a cell. `✓` and `✗` are missing from
Consolas, so the classic console host may draw a box instead; Windows
Terminal's default Cascadia fonts have all five.

The `kind`, `progress`, `app`, `title` and `msg` variables describe the same
record that `#{pane_agent_state}` shows. Their text is cleaned before it is
shown outside the pane: the parser already rejects control characters, and the
server also removes bidirectional overrides (U+202A-202E, U+2066-2069) and
invisible format characters (U+200B-200F, U+FEFF) and keeps at most 200
characters. When the pane's process exits, `working`, `blocked` and `idle`
records are dropped and `done` and `error` survive. They stay visible only
while the dead pane is kept (`remain-on-exit`); otherwise they go with the pane.

`#{pane_ready}`, `wait-pane --ready`, `wait-for --ready`,
`send-keys --wait-ready`, the JSON-RPC `spawn_agent` `wait_ready` option and
the `context_ready` event all use one rule. A pane with an OSC 7501 record
takes its reported state (`#{pane_agent_source}` is `reported`); a pane with
none falls back to 500 ms of output silence (`output_stable`). After
`state=clear` the pane falls back to silence again. An agent that redraws a
spinner while it reports `idle` is ready; one that reports `working` while
silent is not. The reported state is the worst over the `idle`, `working` and
`blocked` records, so a Claude Code background task that is `working` keeps
the pane not ready while the main session is `idle`.

`done` and `error` records survive the end of the program, so they do not
decide readiness, even when `#{pane_agent_state}` shows one: a leftover
`error` child does not hide a `working` root, and a leftover `done` child does
not hide an `idle` one. With only those left, the pane is judged by output
silence.
A `working` or `blocked` record does decide, and an agent that crashes or is
killed never sends `state=clear`, so its leftover record keeps the pane not
ready: `#{pane_ready}` stays 0, `wait-pane --ready` without `--timeout` never
returns and `send-keys --wait-ready` waits its full 30 seconds. psmux does not
inject OSC 133 prompts, so the record goes only when the pane's process exits,
the shell prompt emits OSC 133 A, or the terminal is reset. To recover, run
`psmux clear-history -t <pane>`, or print `ESC c` in the pane (for example
`[Console]::Write([char]27 + 'c')` in pwsh), or send `state=clear` yourself (with `:id=<id>` for a child record; a bare
`state=clear` removes only the root record).

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
