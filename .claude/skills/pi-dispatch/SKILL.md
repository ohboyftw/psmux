---
name: pi-dispatch
description: >
  Bridge Pi coding agents into Claude Code's TeammateTool system. Use when
  spawning Pi agents as part of a swarm, routing tasks to Pi instead of Claude
  Code, or discussing multi-model agent orchestration. Triggers on "pi agent",
  "pi dispatch", "pi -p", "pi coding agent", "multi-model swarm", "use pi for
  this task", or when the leader decides a task is simple enough for Pi.
---

# Pi Agent Dispatch Bridge

Dispatch Pi coding agents from Claude Code sessions via psmux panes.
Two scripts handle single-agent and multi-agent dispatch.

## Scripts

| Script | Location | Purpose |
|--------|----------|---------|
| `pi-dispatch.ps1` | `.claude/scripts/pi-dispatch.ps1` | Single Pi agent in a psmux pane |
| `pi-swarm.ps1` | `.claude/scripts/pi-swarm.ps1` | N parallel Pi agents in psmux panes |

Also available in Canopy: `D:\Home\canopy\scripts\`

## Quick Start

### Single agent (from Claude Code Bash tool)

```bash
pwsh .claude/scripts/pi-dispatch.ps1 -Prompt "fix clippy warning in pane.rs" -WorkDir D:/Home/psmux -Timeout 120
```

### Parallel swarm (from Claude Code Bash tool)

```bash
# Create a tasks JSON file
cat > /tmp/tasks.json << 'EOF'
[
  {"name": "analyze", "prompt": "analyze src/main.rs for error handling gaps"},
  {"name": "tests", "prompt": "list all untested public functions in src/types.rs"},
  {"name": "docs", "prompt": "check which public functions lack doc comments in src/pane.rs"}
]
EOF

pwsh .claude/scripts/pi-swarm.ps1 -Tasks /tmp/tasks.json -Timeout 300
```

## How It Works

### pi-dispatch.ps1

1. Creates a psmux pane (`split-window -d -h`)
2. Sends `cd` + `pi -p '<prompt>'` via `send-keys` (Pi gets a real TTY)
3. Polls for completion via marker file + pane liveness check
4. Captures output via `capture-pane`
5. Kills pane and returns result

### pi-swarm.ps1

1. Creates N psmux panes (one per task from JSON)
2. Sends Pi command to each pane via `send-keys`
3. Each task writes output to a file + creates a `.done` marker
4. Polls all markers in parallel
5. Collects results and returns JSON with per-task status

### Key Learnings (from 2026-03-17 session)

- Pi `-p` flag = non-interactive mode, works in psmux panes via send-keys
- **Do NOT use file redirect (`>`) with Pi** — it blocks Pi's tool access
- Use marker files (`.done`) for completion detection, not capture-pane polling
- Pi needs a real TTY — background Bash subagents don't provide one
- The `UV_HANDLE_CLOSING` assertion error is harmless Node.js cleanup noise

## Integration Points

### From Claude Code Agent tool

Pi can't run via `Agent` tool directly (no TTY). Use Bash tool instead:

```bash
# Single dispatch
pwsh .claude/scripts/pi-dispatch.ps1 -Prompt "task" -Timeout 120

# Swarm dispatch
pwsh .claude/scripts/pi-swarm.ps1 -Tasks tasks.json -Timeout 300
```

### From Claude Code TeammateTool

Use `send-keys` via psmux to inject Pi into a team pane:

```bash
psmux split-window -d -h -t team-session -P -F "#{pane_id}"
# → returns %N
psmux send-keys -t %N "cd /path && pi -p 'task prompt'" Enter
```

Monitor via `psmux capture-pane -t %N -p` or marker files.

### From Canopy

Canopy's `spawner.py` has native `PI_SWARM` support:
- Router detects decomposable tasks (bullet points or `#swarm` tag)
- Spawner calls `pi-swarm.ps1` with subtasks extracted from task description
- Monitor watches `.canopy-done` / `.canopy-failed` sentinel files
- No changes needed to Canopy's monitor or deliver stages

## When to Use Pi vs Claude Code

| Task | Agent | Why |
|------|-------|-----|
| Single-file focused fix | Pi | Fast, cheap |
| Multi-file architecture | Claude Code | Broad context needed |
| Bug fix with known location | Pi | Surgical |
| Code review / safety audit | Claude Code | Judgment-heavy |
| Test writing (unit) | Pi | Formulaic |
| Test writing (integration) | Claude Code | Cross-file context |
| Research / exploration | Claude Code (Explore) | Read-only, fast |

## Provider Routing

Spread Pi agents across providers to avoid rate limits:

```powershell
# Default provider (configured in Pi settings)
pwsh pi-dispatch.ps1 -Prompt "task 1"

# Override per-swarm
pwsh pi-swarm.ps1 -Tasks tasks.json -Provider ollama

# Per-agent in swarm (set in task JSON or env)
[{"name": "t1", "prompt": "...", "provider": "anthropic"},
 {"name": "t2", "prompt": "...", "provider": "ollama"}]
```

## Canopy Integration Architecture

```
Canopy loop.py
  └─ router.py: score < 0.4 → PI, 0.3-0.6 + bullets → PI_SWARM, >= 0.7 → CLAUDE_CODE
      └─ spawner.py
          ├─ PI: psmux send-keys → pi -p "$(cat .canopy-prompt)"
          ├─ PI_SWARM: psmux send-keys → pwsh pi-swarm.ps1 -Tasks .canopy-swarm-tasks.json
          └─ CLAUDE_CODE: psmux send-keys → claude -p "$(cat .canopy-prompt)"
```
