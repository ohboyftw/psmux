# CustomPaneBackend — JSON-RPC Protocol Reference

The CustomPaneBackend is a named-pipe JSON-RPC server that psmux starts for
every session. It is the integration point for Claude Code's TeammateTool,
the Pi coding agent's PsmuxAdapter, and any other tool that needs programmatic
control over psmux panes.

## Transport

**Pipe path:** `\\.\pipe\psmux-claude-backend-{session_name}`

**Discovery file:** `%USERPROFILE%\.psmux\{session_name}.pipe` (written at
session start; contains the pipe path as a plain string). Reading this file is
the preferred way to locate the pipe — no guessing the session name required.

**Framing:** newline-delimited JSON. Each request is one JSON object on a
single line (`\n` terminated). Each response is one JSON object on a single
line. Push events from the server arrive on the same connection as
additional newline-delimited JSON objects; they have a `"method"` field at
top level and no `"id"`.

**Environment variables that set the socket path (for legacy clients):**

| Variable | Consumed by |
|----------|-------------|
| `CLAUDE_PANE_BACKEND_SOCKET` | Claude Code TeammateTool |
| `PI_PANE_BACKEND_SOCKET` | Pi coding agent |

These variables are set by external tooling; psmux itself uses the
`%USERPROFILE%\.psmux\{session}.pipe` discovery file directly.

**Who writes first:** the client sends a request; the server never writes
unsolicited data except for push events (see [Push Events](#push-events)).
Send `initialize` first.

---

## Request / Response Envelope

Every request:
```json
{"id": "any-string-or-number", "method": "method_name", "params": { ... }}
```

Every success response:
```json
{"id": "same-as-request", "result": { ... }}
```

Every error response:
```json
{"id": "same-as-request", "error": {"code": -32001, "message": "...", "data": { ... }}}
```

`data` is omitted when there is no extra context. `id` echoes the request id;
pass `null` for notifications (not used today — all methods expect a response).

---

## Error Codes

| Code | Constant | Meaning |
|------|----------|---------|
| -32700 | — | Parse error — malformed JSON |
| -32601 | — | Method not found |
| -32602 | — | Invalid params |
| -32603 | — | Internal server error |
| -32001 | `PANE_NOT_FOUND` | No pane with that context_id |
| -32002 | `SPAWN_FAILED` | Pane could not be created |
| -32003 | `PANE_TOO_SMALL` | Terminal too small to split |
| -32004 | `SPAWN_TIMEOUT` | Pane spawned but never reached idle prompt |
| -32005 | `CAPTURE_TIMEOUT` | No new output within wait window |
| -32006 | `SESSION_NOT_FOUND` | Named session does not exist |
| -32007 | `COMMAND_TIMEOUT` | Command exceeded timeout_ms |
| -32008 | `COMMAND_FAILED` | Command failed to spawn or exited with error |

---

## Methods

### `initialize`

Handshake. Must be the first call. Returns the protocol version and the
context_id of the pane that owns the current connection.

**Request params:**
```json
{
  "protocol_version": "2",
  "capabilities": ["events", "capture"]
}
```

**Response result:**
```json
{
  "protocol_version": "2",
  "capabilities": ["events", "capture", "run_shell"],
  "self_context_id": "%3"
}
```

`self_context_id` is the `%N` pane id of the caller's pane (the pane that
opened this connection). `capabilities` lists what the server supports.

---

### `spawn_agent`

Create a new pane running a command. Optionally wait until the pane reaches
an idle shell prompt before returning.

**Request params:**
```json
{
  "command": ["claude", "--agent-id", "abc123"],
  "cwd": "D:/project",
  "env": {"MY_VAR": "value"},
  "metadata": {
    "name": "worker-1",
    "role": "coder",
    "color": "blue",
    "effort": "high",
    "max_turns": 20,
    "disallowed_tools": ["BashTool"]
  },
  "split_direction": "vertical",
  "mode": "auto",
  "window_name": "agent",
  "wait_ready": true,
  "ready_timeout_ms": 15000,
  "shell": "bash",
  "bare": false
}
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `command` | `string[]` | required | Argv. Empty array spawns a default shell pane. |
| `cwd` | `string?` | null | Working directory for the new pane. |
| `env` | `object?` | null | Extra environment variables. `CLAUDE_CODE_NO_FLICKER=1` is always injected. |
| `metadata` | object? | null | Agent metadata stored on the pane (see `@agent`, `@role`, etc.). |
| `split_direction` | `"horizontal"\|"vertical"?` | null | Split orientation when mode is `"split"`. |
| `mode` | `"split"\|"window"\|"auto"` | `"auto"` | `"auto"` tries split, falls back to new window if pane is too small. |
| `window_name` | `string?` | null | Window name when mode is `"window"` or auto-fallback. |
| `wait_ready` | `bool` | `true` | Block until the pane reaches an idle prompt. |
| `ready_timeout_ms` | `number?` | 15000 | Max wait for readiness. Returns `-32004` on timeout; `context_id` is in `error.data`. |
| `shell` | `string?` | null | Override the shell used to launch the pane (`bash`, `pwsh`, `cmd`). |
| `bare` | `bool?` | false | Prepend `--bare` to a claude command (flicker-free rendering). |

**Response result:**
```json
{
  "context_id": "%7",
  "ready": true,
  "elapsed_ms": 1240,
  "data_version": 3,
  "created_via": "split"
}
```

`created_via` is `"split"` or `"window"`.

---

### `list`

Return all active pane contexts.

**Request params:** `{}`

**Response result:**
```json
{
  "contexts": [
    {
      "context_id": "%1",
      "alive": true,
      "cwd": "D:/project",
      "title": "pwsh",
      "shell_name": "pwsh",
      "metadata": null
    },
    {
      "context_id": "%3",
      "alive": true,
      "cwd": "D:/project/agent",
      "title": "worker-1",
      "shell_name": "claude",
      "metadata": {"name": "worker-1", "role": "coder", "color": "blue",
                   "effort": null, "max_turns": null, "disallowed_tools": null}
    }
  ]
}
```

`cwd`, `title`, and `shell_name` are omitted when not available.

---

### `capture`

Read the visible screen content of a pane.

**Request params:**
```json
{
  "context_id": "%3",
  "lines": 50,
  "clean": false,
  "wait_for_output": true,
  "since_version": 2,
  "timeout_ms": 5000
}
```

| Field | Default | Notes |
|-------|---------|-------|
| `lines` | all | Max lines to return from the bottom. |
| `clean` | false | Strip ANSI escape sequences. |
| `wait_for_output` | false | Block until `data_version` exceeds `since_version`. |
| `since_version` | null | Baseline version for freshness check. |
| `timeout_ms` | 5000 | How long to wait. On timeout returns `-32005`; stale text is in `error.data`. |

**Response result:**
```json
{
  "text": "$ npm test\n... 42 passing\n$",
  "data_version": 5,
  "context_id": "%3"
}
```

`data_version` increments with every write to the pane. Use `since_version`
to poll for new output without re-reading stale content.

---

### `write`

Send text to a pane as if typed by the user.

**Request params:**
```json
{
  "context_id": "%3",
  "data": "aGVsbG8K"
}
```

`data` is **base64-encoded** bytes. The decoded value is forwarded verbatim
to the pane's PTY. Include `\n` (0x0A) to send Enter.

**Response result:** `{}`

**Example — send `cargo test\n`:**
```json
{"id": 1, "method": "write", "params": {"context_id": "%3", "data": "Y2FyZ28gdGVzdAo="}}
```

---

### `exec`

Run a shell command in the context of a pane (inherits its live CWD and
environment). Runs on a background thread. Fires an `exec_completed` push
event when done.

**Request params:**
```json
{
  "context_id": "%3",
  "command": "git status",
  "capture": true,
  "timeout_ms": 30000,
  "shell": "bash"
}
```

| Field | Default | Notes |
|-------|---------|-------|
| `context_id` | null | Target pane. `null` uses the active pane. |
| `command` | required | Command string to execute. |
| `capture` | false | Include `stdout`/`stderr` in the result. |
| `timeout_ms` | 30000 | Kill the command and return `-32007` on timeout. |
| `shell` | null | Override the shell used to run the command. |

**Response result:**
```json
{
  "exit_code": 0,
  "stdout": "On branch main\nnothing to commit\n",
  "stderr": "",
  "elapsed_ms": 87
}
```

`stdout` and `stderr` are omitted when `capture` is false.

---

### `run_shell`

Spawn a process server-side and collect stdout/stderr. Unlike `exec`, this
runs the command directly (not through a pane's PTY) and returns output
synchronously. CWD is resolved from `cwd` param, then the pane's spawn_cwd,
then nothing.

**Request params:**
```json
{
  "command": ["git", "log", "--oneline", "-5"],
  "cwd": "D:/project",
  "context_id": "%3",
  "timeout_ms": 30000,
  "env": {"GIT_PAGER": "cat"}
}
```

`command` must be non-empty. `context_id` is used only to resolve CWD if
`cwd` is omitted.

**Response result:**
```json
{
  "exit_code": 0,
  "stdout": "abc1234 fix typo\ndef5678 add feature\n",
  "stderr": "",
  "elapsed_ms": 43
}
```

On timeout (`-32007`), partial stdout/stderr are available in `error.data`.

---

### `wait_for`

Block until a condition is met, then return. Replaces polling loops.

**Request params:**
```json
{
  "condition": "output",
  "arg": "\\$\\s*$",
  "pane_id": "%3",
  "timeout_ms": 60000
}
```

| `condition` | `arg` | Notes |
|------------|-------|-------|
| `"exit"` | PID (number as string) | Wait for a process to exit by PID. Windows only. |
| `"file"` | Absolute path | Wait for a file to appear on disk. |
| `"output"` | Regex pattern | Wait for the pattern to match the pane's live screen. Requires `pane_id`. |
| `"ready"` | — | Wait for the pane to reach an idle shell prompt. Requires `pane_id`. |

Default `timeout_ms` is 3 600 000 (1 hour).

**Response result — success:**
```json
{"kind": "success", "elapsed_ms": 1240}
```

**Response result — exit condition:**
```json
{"kind": "exit_success", "elapsed_ms": 340, "exit_code": 0}
```

**Response result — timeout:**
```json
{"kind": "timeout", "elapsed_ms": 60000}
```

**Response result — error:**
```json
{"kind": "error", "reason": "wait_for exit is only supported on Windows"}
```

---

### `kill`

Terminate a pane.

**Request params:**
```json
{"context_id": "%3", "grace_ms": 500}
```

`grace_ms` is optional. **Response result:** `{}`

---

### `kill_all`

Kill all agent-spawned panes, optionally filtered by role.

**Request params:**
```json
{"role": "coder"}
```

`role` is optional; omit to kill all. **Response result:**
```json
{"killed": ["%3", "%5"]}
```

---

### `set_metadata`

Update metadata on an existing pane.

**Request params:**
```json
{
  "context_id": "%3",
  "metadata": {"name": "worker-1", "role": "coder", "color": null,
               "effort": null, "max_turns": null, "disallowed_tools": null}
}
```

**Response result:** `{}` — returns `-32001` if the pane is not found.

---

## Push Events

Push events are server-initiated. They arrive on the same pipe connection as
responses, as newline-delimited JSON objects with a `"method"` field and no
`"id"`. No subscription call is needed — push events are sent to all connected
clients automatically.

### `context_exited`

Fired when a pane's process terminates.

```json
{
  "method": "context_exited",
  "params": {
    "context_id": "%3",
    "exit_code": 0,
    "elapsed_ms": 4210,
    "command": "claude --agent-id abc123"
  }
}
```

`elapsed_ms` and `command` are omitted when not available (e.g. pane killed
externally).

### `context_ready`

Fired when a pane reaches an idle shell prompt for the first time after
spawning.

```json
{
  "method": "context_ready",
  "params": {
    "context_id": "%3",
    "ready_signal": "idle",
    "data_version": 2
  }
}
```

### `exec_completed`

Fired after every `exec` call completes (success or failure).

```json
{
  "method": "exec_completed",
  "params": {
    "context_id": "%3",
    "exit_code": 0,
    "command": "git status",
    "elapsed_ms": 87
  }
}
```

---

## Complete Example — spawn an agent and wait for output

```jsonc
// 1. Handshake
→ {"id":1,"method":"initialize","params":{"protocol_version":"2","capabilities":[]}}
← {"id":1,"result":{"protocol_version":"2","capabilities":["events","capture","run_shell"],"self_context_id":"%1"}}

// 2. Spawn a new pane
→ {"id":2,"method":"spawn_agent","params":{"command":["pwsh","-NoLogo"],"wait_ready":true,"mode":"split"}}
← {"id":2,"result":{"context_id":"%3","ready":true,"elapsed_ms":980,"data_version":1,"created_via":"split"}}

// 3. Run a command in it
→ {"id":3,"method":"exec","params":{"context_id":"%3","command":"cargo test","capture":true,"timeout_ms":120000}}
// Server may send push events before the response:
← {"method":"exec_completed","params":{"context_id":"%3","exit_code":0,"command":"cargo test","elapsed_ms":4301}}
← {"id":3,"result":{"exit_code":0,"stdout":"test result: ok. 42 passed\n","stderr":"","elapsed_ms":4301}}

// 4. Capture final screen
→ {"id":4,"method":"capture","params":{"context_id":"%3","clean":true}}
← {"id":4,"result":{"text":"test result: ok. 42 passed\n$","data_version":4,"context_id":"%3"}}

// 5. Tear down
→ {"id":5,"method":"kill","params":{"context_id":"%3"}}
← {"id":5,"result":{}}
// Push event arrives after kill:
← {"method":"context_exited","params":{"context_id":"%3","exit_code":-1}}
```
