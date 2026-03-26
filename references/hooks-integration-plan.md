# Claude Code Hooks Integration Plan for psmux

**Date:** 2026-03-26
**Branch:** ohboy-builds
**Status:** Planning (no code changes)

## Background

Claude Code v2.1.83+ introduced hook events that fire at lifecycle points during a
session. Hooks are user-defined shell commands or HTTP endpoints configured in
`.claude/settings.json`. psmux already serves as the tmux spawn backend for
Claude Code's TeammateTool on Windows via its CustomPaneBackend (JSON-RPC over
named pipes at `\\.\pipe\psmux-claude-backend-{session}`). These new hook events
create opportunities to tighten the integration.

### Relevant Hook Events

| Event | Blocking | Input Fields | Key Output |
|-------|----------|--------------|------------|
| `WorktreeCreate` | Yes | `session_id`, `cwd` | `hookSpecificOutput.worktreePath` |
| `TaskCreated` | Yes | `task_id`, `task_subject`, `task_description`, `teammate_name`, `team_name` | `continue`, exit code 2 blocks |
| `TaskCompleted` | Yes | `task_id`, `task_subject`, `teammate_name`, `team_name` | `continue` |
| `CwdChanged` | No | `old_cwd`, `new_cwd` | `hookSpecificOutput.watchPaths` |
| `FileChanged` | No | `file_path`, `event` (change/add/unlink) | `hookSpecificOutput.watchPaths` |

### psmux Capabilities Referenced

- **CustomPaneBackend**: JSON-RPC named pipe server (`src/backend/`), protocol v2
- **Warm pane pool**: Pre-spawned shells for instant session creation
- **OSC 99/777 notifications**: Desktop toast notifications from pane output (`src/server/helpers.rs`)
- **mycel event bus**: Optional pub/sub for pane lifecycle events (`src/mycel.rs`)
- **Named pipe discovery**: `~/.psmux/{session}.pipe` files
- **Agent metadata**: `@agent`, `@role`, `@color` pane-level options
- **`run_shell` RPC method**: Server-side command execution with cwd resolution

---

## 1. WorktreeCreate HTTP Hook

### Problem

When Claude Code's leader agent spawns a teammate, it creates a git worktree for
isolation. On Windows, this currently happens inside Claude Code's own logic.
psmux has no awareness of worktrees, so each agent ends up in a generic pane with
no session-level isolation.

### Design: psmux as Worktree Orchestrator

Intercept the `WorktreeCreate` hook to have psmux create both the worktree AND
a dedicated session/window for it, returning the path so Claude Code uses it.

#### Option A: Command Hook (PowerShell script, no Rust changes)

A PowerShell script that reads the hook input from stdin, calls `git worktree add`,
creates a psmux window for it, and returns the path.

**File:** `.claude/hooks/worktree-create.ps1`

```powershell
# Read hook input from stdin
$input_json = [Console]::In.ReadToEnd() | ConvertFrom-Json

$session_id = $input_json.session_id
$cwd = $input_json.cwd
$teammate = $input_json.teammate_name  # may be null for non-team worktrees

# Generate worktree path
$repo_root = & git -C $cwd rev-parse --show-toplevel 2>$null
if (-not $repo_root) { exit 1 }

$branch_name = "wt/$teammate-$(Get-Random -Maximum 9999)"
$wt_path = Join-Path $repo_root ".worktrees" $branch_name

# Create the worktree
& git -C $repo_root worktree add -b $branch_name $wt_path HEAD 2>$null
if ($LASTEXITCODE -ne 0) {
    [Console]::Error.WriteLine("Failed to create worktree at $wt_path")
    exit 2
}

# Create a psmux window for this worktree
$psmux_session = $env:PSMUX_SESSION
if ($psmux_session) {
    $win_name = if ($teammate) { $teammate } else { (Split-Path $wt_path -Leaf) }
    & psmux new-window -t "$psmux_session" -n $win_name -c $wt_path 2>$null
}

# Return the worktree path to Claude Code
@{
    hookSpecificOutput = @{
        worktreePath = $wt_path -replace '\\', '/'
    }
} | ConvertTo-Json -Compress
exit 0
```

**Configuration** (`.claude/settings.json` or `~/.claude/settings.json`):

```json
{
  "hooks": {
    "WorktreeCreate": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "powershell -NoProfile -File \"$CLAUDE_PROJECT_DIR/.claude/hooks/worktree-create.ps1\"",
            "timeout": 30
          }
        ]
      }
    ]
  }
}
```

#### Option B: HTTP Hook (psmux-hosted endpoint, requires Rust changes)

psmux's server loop already has a TCP listener for client connections. Add an
HTTP endpoint that handles worktree creation directly within the server process.

**Endpoint:** `POST http://localhost:{psmux_port}/hooks/worktree-create`

**Request body:** Claude Code's `WorktreeCreate` hook input JSON.

**Response:**
```json
{
  "hookSpecificOutput": {
    "worktreePath": "D:/Home/psmux/.worktrees/wt/auth-1234"
  }
}
```

**Server-side behavior:**
1. Parse `teammate_name` and `cwd` from input
2. Run `git worktree add` via `std::process::Command`
3. Create a new psmux window (`CtrlReq::NewWindow`) with `start_dir` set to worktree path
4. Set `@agent` metadata on the new pane
5. Return the worktree path

**Configuration:**
```json
{
  "hooks": {
    "WorktreeCreate": [
      {
        "hooks": [
          {
            "type": "http",
            "url": "http://localhost:${PSMUX_PORT}/hooks/worktree-create",
            "timeout": 30
          }
        ]
      }
    ]
  }
}
```

Note: The `${PSMUX_PORT}` would need to be resolved. Since HTTP hooks support
`allowedEnvVars`, the configuration could use:

```json
{
  "type": "http",
  "url": "http://localhost:$PSMUX_PORT/hooks/worktree-create",
  "allowedEnvVars": ["PSMUX_PORT"]
}
```

psmux already sets `PSMUX_SESSION` in pane environments; it would need to also
export `PSMUX_PORT` (the TCP control port from `~/.psmux/{session}.port`).

#### Worktree Cleanup via WorktreeRemove

Pair with a `WorktreeRemove` hook to clean up the psmux window:

```json
{
  "hooks": {
    "WorktreeRemove": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "powershell -NoProfile -File \"$CLAUDE_PROJECT_DIR/.claude/hooks/worktree-remove.ps1\"",
            "timeout": 15
          }
        ]
      }
    ]
  }
}
```

The removal script would find the psmux window associated with the worktree path
(via `psmux list-panes -F '#{pane_current_path} #{pane_id}'`) and kill it.

#### Recommendation

**Start with Option A** (PowerShell command hook). It requires zero Rust changes,
can be iterated on quickly, and validates the integration pattern. Move to Option B
only if latency becomes a problem (unlikely -- `git worktree add` dominates).

---

## 2. TaskCreated Hook

### Problem

When the leader agent creates tasks for teammates, there is no visibility into
the task queue from the psmux UI. The user sees panes but not what each agent
is working on or the overall progress.

### Design: Task Dashboard Pane

#### 2a. Task Event Logging (Command Hook, no Rust changes)

A hook script that appends task events to a JSON log file, which a dashboard
pane can tail.

**File:** `.claude/hooks/task-event.ps1`

```powershell
$input_json = [Console]::In.ReadToEnd() | ConvertFrom-Json

$event = @{
    timestamp = (Get-Date -Format o)
    event     = $input_json.hook_event_name  # "TaskCreated" or "TaskCompleted"
    task_id   = $input_json.task_id
    subject   = $input_json.task_subject
    teammate  = $input_json.teammate_name
    team      = $input_json.team_name
}

$log_dir = Join-Path $env:USERPROFILE ".psmux" "tasks"
New-Item -ItemType Directory -Path $log_dir -Force | Out-Null
$log_file = Join-Path $log_dir "events.jsonl"

$event | ConvertTo-Json -Compress | Add-Content -Path $log_file

# Optionally trigger a psmux status-bar update
if ($env:PSMUX_SESSION) {
    $active = ($input_json.hook_event_name -eq "TaskCreated")
    $count_file = Join-Path $log_dir "active_count.txt"
    if ($active) {
        $current = if (Test-Path $count_file) { [int](Get-Content $count_file) } else { 0 }
        ($current + 1) | Set-Content $count_file
    } else {
        $current = if (Test-Path $count_file) { [int](Get-Content $count_file) } else { 1 }
        [Math]::Max(0, $current - 1) | Set-Content $count_file
    }
}

exit 0
```

**Configuration:**
```json
{
  "hooks": {
    "TaskCreated": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "powershell -NoProfile -File \"$CLAUDE_PROJECT_DIR/.claude/hooks/task-event.ps1\"",
            "timeout": 5
          }
        ]
      }
    ],
    "TaskCompleted": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "powershell -NoProfile -File \"$CLAUDE_PROJECT_DIR/.claude/hooks/task-event.ps1\"",
            "timeout": 5
          }
        ]
      }
    ]
  }
}
```

**Dashboard pane:** A simple script that tails the JSONL file and renders a table:

```powershell
# .claude/hooks/task-dashboard.ps1 — run in a dedicated psmux pane
$log = Join-Path $env:USERPROFILE ".psmux" "tasks" "events.jsonl"
Get-Content -Path $log -Wait -Tail 20 | ForEach-Object {
    $e = $_ | ConvertFrom-Json
    $icon = if ($e.event -eq "TaskCreated") { "[+]" } else { "[x]" }
    "$icon $($e.subject) -> $($e.teammate)"
}
```

Launch via: `psmux split-window -v -p 20 "powershell -NoProfile -File .claude/hooks/task-dashboard.ps1"`

#### 2b. mycel Event Bus Integration (requires mycel feature enabled)

If mycel is available, the hook script can also publish to the event bus for
external subscribers:

```powershell
# In task-event.ps1, after logging to file:
if ($env:MYCEL_SERVER) {
    # Publish to mycel topic (requires mycel CLI or HTTP endpoint)
    $topic = "psmux/tasks/$($input_json.hook_event_name.ToLower())"
    $payload = $event | ConvertTo-Json -Compress
    # mycel publish $topic $payload  (if CLI available)
}
```

Alternatively, psmux's Rust-side `publish_pane_event()` could be extended to
publish task events when it receives them via a new `CtrlReq` variant. The hook
script would send a `CtrlReq` via the named pipe backend:

```json
{"id": "task-1", "method": "publish_event", "params": {
  "topic": "psmux/tasks/created",
  "payload": {"task_id": "task-001", "subject": "Implement OAuth", "teammate": "builder-1"}
}}
```

This would require adding a `publish_event` RPC method to `src/backend/dispatcher.rs`.

#### 2c. Task-Aware Pane Naming

The `TaskCreated` hook includes `teammate_name`. When a worktree pane already
exists for that teammate, update its window name to include the task subject:

```powershell
# In task-event.ps1, after logging:
if ($env:PSMUX_SESSION -and $input_json.teammate_name) {
    $short_subject = $input_json.task_subject.Substring(0, [Math]::Min(30, $input_json.task_subject.Length))
    & psmux rename-window -t "$($env:PSMUX_SESSION):$($input_json.teammate_name)" "$short_subject"
}
```

---

## 3. CwdChanged Hook

### Problem

When a Claude Code agent changes its working directory (via `cd` in a Bash tool
call), psmux has no visibility into this. Window names stay static, and there is
no way to correlate which project directory an agent pane is working in.

### Design: Session-Aware CWD Tracking

#### 3a. Auto-Rename Windows (Command Hook, no Rust changes)

Rename the psmux window to reflect the current working directory whenever the
CWD changes.

**File:** `.claude/hooks/cwd-changed.ps1`

```powershell
$input_json = [Console]::In.ReadToEnd() | ConvertFrom-Json

$new_cwd = $input_json.new_cwd
$session = $env:PSMUX_SESSION

if (-not $session) { exit 0 }

# Extract a short name from the path (last 2 components)
$parts = $new_cwd -split '[/\\]' | Where-Object { $_ }
$short = if ($parts.Count -ge 2) {
    "$($parts[-2])/$($parts[-1])"
} else {
    $parts[-1]
}

# Rename current window
& psmux rename-window -t "$session" "$short"

# Set watchPaths so FileChanged fires for key config files in new CWD
@{
    hookSpecificOutput = @{
        watchPaths = @(
            (Join-Path $new_cwd ".env"),
            (Join-Path $new_cwd "Cargo.toml"),
            (Join-Path $new_cwd "package.json")
        )
    }
} | ConvertTo-Json -Compress
exit 0
```

**Configuration:**
```json
{
  "hooks": {
    "CwdChanged": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "powershell -NoProfile -File \"$CLAUDE_PROJECT_DIR/.claude/hooks/cwd-changed.ps1\"",
            "timeout": 5,
            "async": true
          }
        ]
      }
    ]
  }
}
```

Note: `CwdChanged` is non-blocking, so `"async": true` is appropriate to avoid
adding latency to the agent's workflow.

#### 3b. Session Routing by Project Directory

For multi-project workflows, CWD changes could route agents to different psmux
sessions. If a teammate's worktree is in `/projects/backend/` vs
`/projects/frontend/`, they could be placed in separate sessions.

This is a more advanced pattern that would require:
1. A mapping file (`.claude/hooks/project-sessions.json`) defining CWD-prefix-to-session rules
2. The hook script checking the mapping and calling `psmux move-window` if needed

**Example mapping:**
```json
{
  "routes": [
    { "prefix": "D:/Home/psmux/crates/", "session": "psmux-crates" },
    { "prefix": "D:/Home/psmux/src/", "session": "psmux-core" },
    { "prefix": "D:/Home/psmux/tests/", "session": "psmux-tests" }
  ]
}
```

This is lower priority -- window renaming covers 90% of the value.

#### 3c. CWD-Aware psmux Status Bar

psmux's status bar format strings support `#{pane_current_path}`, but this
reflects the PTY-detected CWD (which may lag). The CwdChanged hook provides the
authoritative CWD from Claude Code itself. A hook could write the CWD to a
pane-level option:

```powershell
& psmux set-option -p -t "$session" "@cwd" "$new_cwd"
```

Then the status bar format could reference `#{@cwd}` for an always-accurate
display. This requires no server changes -- pane-level `@`-prefixed options are
already supported via `CtrlReq::SetPaneOption`.

---

## 4. FileChanged Hook (Bonus)

### Problem

When a watched file changes (e.g., `.env`, `Cargo.toml`), there is no feedback
in the psmux UI. Agents may be working with stale configuration.

### Design: Notification on Config File Changes

Pair `FileChanged` with the existing OSC 99/777 toast notification system:

**File:** `.claude/hooks/file-changed.ps1`

```powershell
$input_json = [Console]::In.ReadToEnd() | ConvertFrom-Json

$file = $input_json.file_path
$event = $input_json.event  # "change", "add", "unlink"
$basename = Split-Path $file -Leaf

# Fire a psmux notification (uses OSC 99 internally)
if ($env:PSMUX_SESSION) {
    Write-Host "`e]99;psmux;$basename $event`e\"
}

exit 0
```

The `watchPaths` returned by the `CwdChanged` hook (section 3a) determine which
files trigger `FileChanged`. This creates a natural pipeline:

```
CwdChanged → sets watchPaths → FileChanged fires → toast notification
```

---

## 5. Implementation Priority

### Tier 1: Script-Only (no Rust changes, immediate value)

| Integration | Effort | Impact | Files to Create |
|-------------|--------|--------|-----------------|
| WorktreeCreate command hook | 2 hours | High | `.claude/hooks/worktree-create.ps1` |
| WorktreeRemove command hook | 30 min | Medium | `.claude/hooks/worktree-remove.ps1` |
| CwdChanged window rename | 30 min | Medium | `.claude/hooks/cwd-changed.ps1` |
| TaskCreated/Completed logging | 1 hour | Medium | `.claude/hooks/task-event.ps1` |
| FileChanged notification | 30 min | Low | `.claude/hooks/file-changed.ps1` |
| Hook settings.json config | 30 min | -- | `.claude/settings.json` updates |

**Total: ~5 hours.** All use `type: "command"` hooks with PowerShell scripts.
No Rust compilation needed. Can be tested immediately.

### Tier 2: psmux Server Enhancements (Rust changes, better UX)

| Enhancement | Effort | Impact | Files to Modify |
|-------------|--------|--------|-----------------|
| Export `PSMUX_PORT` in pane env | 15 min | Medium | `src/pane.rs` |
| HTTP hook endpoint in server | 4 hours | High | `src/server/mod.rs`, new `src/server/hooks.rs` |
| `publish_event` RPC method | 2 hours | Medium | `src/backend/dispatcher.rs`, `src/backend/protocol.rs` |
| Task-count status bar variable | 1 hour | Low | `src/format.rs` |

**Total: ~7 hours.** These provide tighter integration but are not required for
the hook scripts to work.

### Tier 3: Advanced Patterns (future work)

| Pattern | Effort | Impact | Depends On |
|---------|--------|--------|------------|
| HTTP-mode WorktreeCreate | 6 hours | High | Tier 2 HTTP endpoint |
| Session routing by CWD | 4 hours | Medium | Tier 1 CwdChanged |
| mycel task event publishing | 2 hours | Low | Tier 2 `publish_event` |
| Live task dashboard pane | 3 hours | Medium | Tier 1 task logging |
| Worktree-per-session isolation | 8 hours | High | Tier 1 worktree hooks |

### Recommended Execution Order

1. **Week 1:** Implement all Tier 1 scripts. Validate with a real swarm session.
2. **Week 2:** Add `PSMUX_PORT` env var export (trivial Rust change). Test HTTP
   hook feasibility.
3. **Week 3:** If HTTP hooks prove valuable, implement the server-side endpoint.
4. **Ongoing:** Tier 3 patterns based on real-world usage feedback.

---

## 6. Settings.json — Complete Hook Configuration

This is the full configuration block that enables all Tier 1 integrations. Add to
the project's `.claude/settings.json`:

```json
{
  "hooks": {
    "WorktreeCreate": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "powershell -NoProfile -File \"$CLAUDE_PROJECT_DIR/.claude/hooks/worktree-create.ps1\"",
            "timeout": 30,
            "statusMessage": "Creating worktree + psmux window..."
          }
        ]
      }
    ],
    "WorktreeRemove": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "powershell -NoProfile -File \"$CLAUDE_PROJECT_DIR/.claude/hooks/worktree-remove.ps1\"",
            "timeout": 15
          }
        ]
      }
    ],
    "TaskCreated": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "powershell -NoProfile -File \"$CLAUDE_PROJECT_DIR/.claude/hooks/task-event.ps1\"",
            "timeout": 5
          }
        ]
      }
    ],
    "TaskCompleted": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "powershell -NoProfile -File \"$CLAUDE_PROJECT_DIR/.claude/hooks/task-event.ps1\"",
            "timeout": 5
          }
        ]
      }
    ],
    "CwdChanged": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "powershell -NoProfile -File \"$CLAUDE_PROJECT_DIR/.claude/hooks/cwd-changed.ps1\"",
            "timeout": 5,
            "async": true
          }
        ]
      }
    ],
    "FileChanged": [
      {
        "matcher": ".env|Cargo.toml|package.json",
        "hooks": [
          {
            "type": "command",
            "command": "powershell -NoProfile -File \"$CLAUDE_PROJECT_DIR/.claude/hooks/file-changed.ps1\"",
            "timeout": 5,
            "async": true
          }
        ]
      }
    ]
  }
}
```

---

## 7. Architecture Diagram

```
Claude Code Session (Leader)
│
├── WorktreeCreate hook ──→ worktree-create.ps1 ──→ git worktree add
│                                                 ──→ psmux new-window -c $wt_path
│                          ◄── { worktreePath }
│
├── TaskCreated hook ─────→ task-event.ps1 ────────→ ~/.psmux/tasks/events.jsonl
│                                                  ──→ psmux rename-window (task subject)
│
├── CwdChanged hook ──────→ cwd-changed.ps1 ───────→ psmux rename-window (short path)
│                                                  ──→ psmux set-option -p @cwd
│                          ◄── { watchPaths }
│
├── FileChanged hook ─────→ file-changed.ps1 ──────→ OSC 99 toast notification
│
└── TeammateTool ─────────→ CustomPaneBackend ─────→ psmux named pipe (JSON-RPC)
    (spawn_agent,           \\.\pipe\psmux-          (existing, unchanged)
     capture, kill)          claude-backend-{s}
```

---

## 8. Open Questions

1. **Hook execution context:** Do hooks run in the leader's CWD or the agent's?
   The `cwd` field in hook input should clarify, but needs testing. If hooks
   always run in the leader's CWD, the scripts need to be path-independent.

2. **`$PSMUX_SESSION` availability in hooks:** Hooks inherit the environment of
   the Claude Code process. If Claude Code was launched inside a psmux pane,
   `PSMUX_SESSION` will be set. If launched outside, the scripts need a fallback
   (read from `~/.psmux/*.pipe` files).

3. **WorktreeCreate input fields:** The hook docs show minimal input. Need to
   confirm whether `teammate_name` or `branch_name` is available in the input,
   or if only `session_id` and `cwd` are provided. The script handles the
   `teammate_name: null` case.

4. **Concurrent hook execution:** If multiple teammates are spawned simultaneously,
   multiple `WorktreeCreate` hooks may fire in parallel. The `git worktree add`
   commands must use unique branch names (handled via random suffix in the script).

5. **HTTP hook environment variable expansion:** The docs show `$VAR` expansion
   in `url` and `headers` fields with `allowedEnvVars`. Need to verify this works
   with `PSMUX_PORT` for the Tier 2 HTTP endpoint approach.
