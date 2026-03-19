# Claude Code tmux Backend — Exact Shell Commands

Extracted from the Claude Code swarm orchestration gist (Klaassen, Jan 2026).
These are the exact tmux commands that Claude Code's spawn backend executes.
psmux must handle all of them identically to real tmux.

## Detection Phase

```bash
# Check if inside tmux
test -n "$TMUX"

# Check if tmux binary is available
which tmux
```

## Team Spawn Phase

```bash
# Create new session (if spawning external tmux session)
tmux new-session -s claude-swarm -d

# Split pane for each teammate, capture pane ID
tmux split-window -h -t claude-swarm -P -F "#{pane_id}"
tmux split-window -v -t claude-swarm -P -F "#{pane_id}"
```

## Agent Injection Phase

```bash
# Send Claude Code command into agent pane
tmux send-keys -t %5 "claude --agent-mode ..." Enter

# Send literal text (no key parsing)
tmux send-keys -l -t %5 "complex prompt with special chars"
```

## Monitoring Phase

```bash
# List all panes to check status
tmux list-panes -t claude-swarm

# Select specific pane for inspection
tmux select-pane -t %5

# Rebalance layout after many splits
tmux select-layout -t claude-swarm tiled
```

## Cleanup Phase

```bash
# Kill specific teammate pane
tmux kill-pane -t %5

# Kill entire swarm session
tmux kill-session -t claude-swarm
```

## Environment Variables Set by Backend

When a teammate is spawned into a tmux pane, these env vars are injected:

```
CLAUDE_CODE_TEAM_NAME=my-project
CLAUDE_CODE_AGENT_ID=worker-1@my-project
CLAUDE_CODE_AGENT_NAME=worker-1
CLAUDE_CODE_AGENT_TYPE=Explore
CLAUDE_CODE_AGENT_COLOR=#4A90D9
CLAUDE_CODE_PLAN_MODE_REQUIRED=false
CLAUDE_CODE_PARENT_SESSION_ID=session-xyz
```

These are set via `send-keys` (env var export commands) before the main prompt,
or via the process environment when spawning. psmux doesn't need to handle
these specially — they're just text sent through `send-keys`.

## Team Config Pane Tracking

The backend stores pane IDs in `~/.claude/teams/{team}/config.json`:

```json
{
  "members": [
    {
      "name": "worker-1",
      "backendType": "tmux",
      "tmuxPaneId": "%5"
    }
  ]
}
```

The `%N` format is critical — psmux must use this exact format for pane IDs.

## Critical Format Requirements

1. **Pane IDs**: Must be `%N` format (e.g., `%0`, `%1`, `%5`)
2. **list-panes output**: Must include `%N` identifiers
3. **split-window -P -F "#{pane_id}"**: Must return `%N` on stdout
4. **$TMUX**: Must be set in child shell environment
5. **Exit codes**: `has-session` returns 0 (exists) or 1 (not found)
