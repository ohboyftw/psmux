# Claude Code Agent Teams on Windows via psmux

psmux is the **first Windows-native tmux alternative** that supports Claude Code's agent teams with visible split panes. No WSL, no Linux VM — native Windows.

## Quick Start

```powershell
# 1. Install psmux (ohboy-builds has latest agent features)
cargo install --git https://github.com/ohboyftw/psmux.git --branch ohboy-builds

# 2. Launch Claude Code with agent teams
pwsh scripts/Start-ClaudeTeams.ps1
```

That's it. Claude Code detects psmux as the tmux backend and spawns each teammate in its own visible pane.

## What Happens

```
┌─────────────────────────────────────────────────────┐
│ psmux session: claude-teams                          │
├───────────────────────┬─────────────────────────────┤
│ %0: Claude Code       │ %1: teammate "researcher"   │
│ (team lead)           │ (auto-spawned by Claude)    │
├───────────────────────┼─────────────────────────────┤
│ %2: teammate "coder"  │ %3: teammate "tester"       │
│ (auto-spawned)        │ (auto-spawned)              │
└───────────────────────┴─────────────────────────────┘
```

When Claude Code spawns agent teams, each teammate appears as a visible psmux pane. You can watch them work in real-time, scroll their output, and even interact with them.

## How It Works

psmux automatically sets these environment variables in every child pane:

| Variable | Value | Purpose |
|----------|-------|---------|
| `TMUX` | `/tmp/psmux-{pid}/default,{port},0` | Claude Code detects tmux backend |
| `TMUX_PANE` | `%0`, `%1`, etc. | Pane identifier |
| `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` | `1` | Enables agent teams feature |

Claude Code sees `$TMUX` is set, finds `tmux` on PATH (psmux aliases as `tmux`), and uses the tmux backend to spawn teammates in split panes. Claude Code reads `teammateMode: "tmux"` from its own `settings.json` and auto-detects `$TMUX` — no injection needed from psmux.

## Features for Agent Teams

### Swarm-Ready Commands

```powershell
# One-shot command execution (new in ohboy-builds)
psmux run "pi -p 'fix the bug'" --capture --clean --timeout 120

# Block until a pane's process exits
psmux wait-pane -t %5 --timeout 60

# Capture pane output without shell noise
psmux capture-pane -t %5 -p --clean

# JSON output for programmatic consumption
psmux list-panes --json
psmux list-sessions --json
```

### Agent Metadata

```powershell
# Tag panes with agent identity
psmux set-option -p @agent "researcher"
psmux set-option -p @task "analyze-codebase"

# Query agent panes
psmux list-panes -F "#{pane_id} #{pane_agent} #{pane_task}"
```

### Warm Pool

```powershell
# Pre-spawn warm sessions for instant agent creation (~50ms)
psmux set -g warm-pool-size 3
```

## Manual Setup (Without Script)

If you prefer to set up manually:

```powershell
# Create a psmux session
psmux new-session -s work

# Inside the psmux session, $TMUX is automatically set
# Now launch Claude Code
claude

# Claude Code will use psmux panes for agent teams
```

## Alternative: Set $TMUX Externally

If you want to run Claude Code outside a psmux session but still use psmux panes:

```powershell
# Start a detached psmux session
psmux new-session -d -s claude-backend

# Get the session info
$port = Get-Content "$env:USERPROFILE\.psmux\claude-backend.port"
$pid = (Get-Process psmux | Select-Object -First 1).Id

# Set TMUX env var
$env:TMUX = "/tmp/psmux-$pid/default,$port,0"
$env:TMUX_PANE = "%0"
$env:CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS = "1"

# Now launch Claude Code — it will use the psmux session for teammates
claude
```

## Pi Agent Integration

psmux also supports dispatching Pi coding agents alongside Claude Code teammates:

```powershell
# Single Pi agent
psmux run "pi -p 'analyze this code'" --capture --clean

# Parallel Pi swarm (3 agents)
pwsh .claude/scripts/pi-swarm.ps1 -Tasks tasks.json

# Mixed swarm: Claude Code teammates + Pi agents in the same session
```

## Compatibility

- **Windows 10/11** — native, no WSL required
- **Windows Terminal** — recommended for best rendering
- **PowerShell 7+** — for scripts
- **Claude Code v2.1.77+** — agent teams support
- **Pi Coding Agent v0.55+** — for multi-model dispatch

## Verified: Agent Teams Demo (March 2026)

Tested agent teams with psmux as the backend. Two teammates ran in parallel and delivered real findings:

```
leader (implicit team)
  ├─ safety-auditor (Explore agent)
  │   → Found 52 unsafe blocks across 5 files, 0 have // SAFETY: comments
  │   → Completed in ~30 seconds
  │
  └─ feature-counter (Explore agent)
      → Found 92/92 tmux-compatible commands fully implemented
      → Categorized: 16 session, 15 window, 16 pane, 10 copy/paste,
        13 config/keys, 9 display/UI, 7 scripting/orchestration, 6 new agent commands
      → Completed in ~30 seconds

Both ran in parallel, reported back, shut down cleanly.
```

### How to Spawn Teammates

Claude Code 2.1.178 removed the `TeamCreate`/`TeamDelete` tools. With
`CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` (psmux sets it), the session has one
implicit team — spawn teammates directly with the `Agent` tool's `name`
parameter. `team_name` is deprecated and ignored.

```python
# 1. Spawn teammates (each gets its own pane when inside psmux)
Agent({
  name: "researcher",
  subagent_type: "Explore",
  prompt: "Find all TODO comments in the codebase",
  run_in_background: true
})

Agent({
  name: "coder",
  subagent_type: "general-purpose",
  prompt: "Implement the auth module",
  run_in_background: true
})

# 2. Message teammates
SendMessage({ to: "coder", message: "Focus on OAuth first", summary: "Prioritize OAuth" })

# 3. Shutdown when done (no separate team-cleanup step — the implicit
#    team goes away with the session)
SendMessage({ to: "coder", message: { type: "shutdown_request" } })
```

## Stats

| Metric | Value |
|--------|-------|
| tmux-compatible commands | **92** (all implemented) |
| Command categories | 7 (session, window, pane, copy, config, display, orchestration) |
| Agent-specific commands | 6 (`wait-pane`, `run`, `capture-pane --clean`, `--json`, `@agent` metadata, warm pool) |
| Unsafe blocks | 52 (Win32 FFI — safety audit pending) |

## Why psmux?

| Feature | tmux (Linux) | psmux (Windows) |
|---------|-------------|-----------------|
| Agent teams backend | Yes | **Yes** |
| Native Windows | No (WSL only) | **Yes** |
| Total commands | ~200 | **92** (all commonly used) |
| Warm session pool | No | **Yes (~50ms spawn)** |
| Agent metadata | No | **Yes (`@agent`, `@task`)** |
| JSON output | No | **Yes (`--json`)** |
| Clean capture | No | **Yes (`--clean`)** |
| `run` command | No | **Yes (one-shot execution)** |
| `wait-pane` | No | **Yes (block until exit)** |
| Pi agent dispatch | Manual | **Built-in scripts** |
