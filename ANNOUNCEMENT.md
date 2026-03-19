# psmux: Windows-Native Agent Teams for Claude Code

**psmux** is the first Windows-native tmux alternative that works as Claude Code's agent teams backend. No WSL. No Linux VM. Native Windows.

## What's New (ohboy-builds)

The `ohboy-builds` branch adds features specifically for multi-agent AI workflows:

### Agent Team Commands
```powershell
# One command to run any tool, wait, and capture output
psmux run "pi -p 'fix the bug'" --capture --clean --timeout 120

# Block until an agent's pane finishes
psmux wait-pane -t %5 --timeout 60

# Get clean output without shell noise
psmux capture-pane -t %5 -p --clean
```

### Structured Output
```powershell
# JSON for programmatic consumption
psmux list-panes --json
psmux list-sessions --json
psmux list-windows --json
psmux display-message --json
psmux capture-pane --json
```

### Agent Metadata
```powershell
# Tag panes with agent identity
psmux set-option -p @agent "researcher"
psmux set-option -p @task "analyze-auth-module"

# Query: which agent is in which pane?
psmux list-panes -F "#{pane_id} #{pane_agent} #{pane_task}"
```

### Warm Pool (~50ms agent spawn)
```powershell
# Pre-spawn N warm sessions for instant agent creation
psmux set -g warm-pool-size 3
```

### Pi Coding Agent Dispatch
```powershell
# Parallel Pi agent swarm in psmux panes
pwsh .claude/scripts/pi-swarm.ps1 -Tasks '[
  {"name": "analyze", "prompt": "audit src/auth.rs"},
  {"name": "test", "prompt": "write tests for parse_args"},
  {"name": "docs", "prompt": "check doc coverage in src/types.rs"}
]'
```

## Quick Start

```powershell
# Install from ohboy-builds
cargo install --git https://github.com/ohboyftw/psmux.git --branch ohboy-builds

# Launch Claude Code with visible agent team panes
pwsh scripts/Start-ClaudeTeams.ps1
```

## The Stack

```
Canopy (orchestration) → routes tasks by complexity
  ├─ Claude Code (complex) → agent teams in psmux panes
  ├─ Pi Coding Agent (focused) → single psmux pane
  └─ Pi Swarm (parallel) → N psmux panes
psmux (runtime) → panes, IPC, metadata, warm pool
```

## Stats

- **92** tmux-compatible commands (all implemented)
- **~50ms** warm session spawn time
- **7** bug fixes from upstream issues
- **6** new agent-specific commands
- **Zero** clippy warnings
- **156** tests passing

## Credits

Built on [psmux](https://github.com/psmux/psmux) by the psmux team.
Agent features developed on the `ohboy-builds` branch at [ohboyftw/psmux](https://github.com/ohboyftw/psmux).

Works with [Claude Code](https://claude.ai/code) agent teams and [Pi Coding Agent](https://github.com/AgenDev/pi).
