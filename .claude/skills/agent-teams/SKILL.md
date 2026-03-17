---
name: agent-teams
description: >
  Launch Claude Code with agent teams via psmux split panes on Windows. Use when
  the user wants to start a multi-agent session with visible panes, spawn agent
  teams, set up the tmux backend, or says "agent teams", "launch teams",
  "visible panes", "split pane agents", "start swarm session", "teammate mode",
  or "Start-ClaudeTeams". Also trigger when diagnosing why agent teams fall back
  to in-process mode on Windows.
---

# Agent Teams via psmux

Launch Claude Code with visible agent team panes on Windows using psmux as the tmux backend.

## When to Use

- User wants to start a Claude Code session with visible agent team panes
- Agent teams are falling back to in-process mode (invisible teammates)
- User wants to set up the tmux backend for Windows
- User asks about `$TMUX` env var or teammate mode

## Quick Launch

```powershell
pwsh scripts/Start-ClaudeTeams.ps1
```

This creates a psmux session, sets `$TMUX` automatically, and launches Claude Code inside it. All teammates get visible split panes.

## Diagnosing In-Process Fallback

If agent teams spawn in-process (invisible) instead of visible panes, check:

```bash
# 1. Is $TMUX set?
echo $TMUX
# Should be: /tmp/psmux-{pid}/default,{port},0
# If empty → you're not inside a psmux session

# 2. Is tmux (psmux) on PATH?
which tmux
tmux -V
# Should return psmux path and version 3.2+

# 3. Is agent teams enabled?
echo $CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS
# Should be: 1

# 4. Is teammate mode set?
echo $PSMUX_CLAUDE_TEAMMATE_MODE
# Should be: tmux (when claude-code-fix-tty is on)
```

## Fix: $TMUX Not Set

You must run Claude Code **inside** a psmux session:

```powershell
# Option A: Use the launcher script
pwsh scripts/Start-ClaudeTeams.ps1

# Option B: Manual
psmux new-session -s work
# Now inside the session, $TMUX is auto-set
claude
```

## Fix: Setting $TMUX Externally

If you can't use the launcher (e.g., IDE terminal):

```powershell
psmux new-session -d -s claude-backend
$port = Get-Content "$env:USERPROFILE\.psmux\claude-backend.port"
$pid = (Get-Process psmux | Select-Object -First 1).Id
$env:TMUX = "/tmp/psmux-$pid/default,$port,0"
$env:TMUX_PANE = "%0"
$env:CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS = "1"
claude
```

## psmux Features for Agent Teams

```bash
# One-shot execution
psmux run "command" --capture --clean --timeout 120

# Block until pane exits
psmux wait-pane -t %N --timeout 60

# Clean output capture
psmux capture-pane -t %N -p --clean

# JSON output
psmux list-panes --json

# Agent metadata
psmux set-option -p @agent "researcher"

# Warm pool for instant spawning
psmux set -g warm-pool-size 3
```

## Reference

Full documentation: [AGENT-TEAMS.md](../../AGENT-TEAMS.md)
