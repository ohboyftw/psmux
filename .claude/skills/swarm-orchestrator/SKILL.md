---
name: swarm-orchestrator
description: >
  Orchestrate multi-agent swarms using psmux + TeammateTool + git worktrees.
  Use when the user wants to run parallel coding agents, spawn a swarm, coordinate
  multiple Claude Code or Pi instances, or says "spawn swarm", "parallel agents",
  "multi-agent", "launch workers", "swarm build", "brrr", or any request to
  split work across multiple agents. Also triggers for "TeammateTool", "team",
  "teammate", "inbox", "task pipeline", or "worktree isolation".
---

# Swarm Orchestration via psmux

You are the leader agent. You coordinate multiple agents working in parallel
through psmux panes, each in its own git worktree.

Read `references/teammatetool-reference.md` for full TeammateTool API details.
Read `references/claude-code-tmux-commands.md` for exact psmux commands to use.

## Leader Workflow

### Phase 1: Decompose

Break the user's request into independent tasks. Each task should be:
- Completable by a single agent in a single session
- Scoped to specific files or modules (avoids merge conflicts)
- Testable independently

### Phase 2: Setup Worktrees

```powershell
# Create worktree per task
git worktree add .worktrees/task-1 -b agent/task-1
git worktree add .worktrees/task-2 -b agent/task-2
git worktree add .worktrees/task-3 -b agent/task-3
```

### Phase 3: Create Team and Tasks

```
Teammate({ operation: "spawnTeam", team_name: "psmux-swarm" })

TaskCreate({ subject: "Task 1 name", description: "Detailed spec", activeForm: "Working..." })
TaskCreate({ subject: "Task 2 name", description: "Detailed spec", activeForm: "Working..." })
TaskCreate({ subject: "Task 3 name", description: "Detailed spec", activeForm: "Working..." })
TaskCreate({ subject: "Review all", description: "Review + merge", activeForm: "Reviewing..." })

# Review depends on all implementation tasks
TaskUpdate({ taskId: "4", addBlockedBy: ["1", "2", "3"] })
```

### Phase 4: Spawn Agents into psmux Panes

```
# Claude Code builder for complex task
Task({
  team_name: "psmux-swarm",
  name: "builder-auth",
  subagent_type: "general-purpose",
  prompt: "cd .worktrees/task-1 && implement OAuth2. Claim task #1, mark complete when done, send summary to team-lead.",
  run_in_background: true
})

# Pi agent for focused task (via pi-dispatch bridge)
Task({
  team_name: "psmux-swarm",
  name: "builder-tests",
  subagent_type: "Bash",
  prompt: "cd .worktrees/task-2 && pi -p 'Write unit tests for parse_args module. When done, write results to ~/.claude/teams/psmux-swarm/inboxes/team-lead.json'",
  run_in_background: true
})

# Claude Code reviewer (waits for implementation to finish)
Task({
  team_name: "psmux-swarm",
  name: "reviewer",
  subagent_type: "general-purpose",
  prompt: "Wait for task #4 to unblock. Review all diffs in .worktrees/task-*. Check for safety, correctness, test coverage. Send assessment to team-lead.",
  run_in_background: true
})
```

### Phase 5: Monitor

```powershell
# Check which agents are alive
psmux list-panes

# Read inbox for results
cat ~/.claude/teams/psmux-swarm/inboxes/team-lead.json

# Check task progress
TaskList()
```

### Phase 6: Merge and Verify

Once reviewer approves:

```powershell
# Merge each worktree
cd /workspace
git merge agent/task-1
git merge agent/task-2
git merge agent/task-3

# Run full test suite
cargo fmt --check && cargo clippy -- -D warnings && cargo test

# Clean up worktrees
git worktree remove .worktrees/task-1
git worktree remove .worktrees/task-2
git worktree remove .worktrees/task-3
```

### Phase 7: Handle Failures

If tests fail after merge:
1. Identify which task introduced the failure
2. Create a new fix task
3. Spawn a new agent to fix it
4. Re-run review and merge

If an agent crashes (5-minute heartbeat timeout):
1. Their tasks remain claimable
2. Spawn a replacement agent
3. It picks up where the failed one left off

### Phase 8: Shutdown

```
Teammate({ operation: "requestShutdown", target_agent_id: "builder-auth" })
Teammate({ operation: "requestShutdown", target_agent_id: "builder-tests" })
Teammate({ operation: "requestShutdown", target_agent_id: "reviewer" })
# Wait for all approvals...
Teammate({ operation: "cleanup" })
```

## Agent Routing Decision

| Task Complexity | Agent | Why |
|----------------|-------|-----|
| Multi-file architecture | Claude Code (general-purpose) | Needs many tools, broad context |
| Single function implementation | Pi agent (via Bash subagent) | Fast, focused, cheap |
| Bug fix with known location | Pi agent | Surgical, single-file |
| Code review / safety audit | Claude Code (general-purpose) | Judgment-heavy, needs full context |
| Codebase exploration | Claude Code (Explore) | Uses Haiku, fast scanning |
| Test writing | Either | Pi for simple tests, Claude for complex integration tests |

## Rate Limit Management

Don't spawn all agents simultaneously. Stagger:
1. Spawn first 2-3 agents
2. Wait for one to complete or go idle
3. Spawn next agent into freed capacity
4. Repeat

Monitor with `psmux list-panes` — if a pane shows rate limit errors in its output
(`psmux capture-pane -t %N -p`), pause spawning for 60 seconds.

## psmux Swarm Capabilities

### Pane Readiness Signal
Before sending commands to a newly-created pane, check if the shell is ready:
```powershell
# Format variable: returns "1" when shell output has stabilised (500ms quiet)
psmux display-message -t %N -p "#{pane_ready}"

# Block until ready (with timeout)
psmux wait-pane -t %N --ready --timeout 10
```

### Headless Mode (Unlimited Agents)
Split panes are limited to ~6-7 per terminal window. For larger swarms, use
**headless windows** — each agent gets its own full 30x120 PTY with no size constraint:
```powershell
# Create detached session
psmux new-session -s swarm -d

# Each agent gets a separate window (not a pane split)
psmux new-window -d -t swarm -P -F "#{pane_id}"   # returns %N
psmux send-keys -t %N "claude -p 'task prompt'" Enter

# Or use pi-swarm.ps1 with -Headless flag
pwsh .claude/scripts/pi-swarm.ps1 -Tasks tasks.json -Headless
```

| Mode | Command | Agents | Use Case |
|------|---------|--------|----------|
| Split panes (visible) | `split-window` | ~6-7 | Monitor in real time |
| Headless windows | `new-window -d` | Unlimited | Large swarms, Pi agents |

### Agent Dispatch Pattern
- **Claude agents → panes** (visible, complex multi-file tasks, max 5-6)
- **Pi agents → headless windows** (unlimited, simple/focused tasks)
- Pi can escalate to Claude sub-agents if tasks turn out complex (Canopy routing)
