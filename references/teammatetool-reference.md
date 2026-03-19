# TeammateTool + Task System — Quick Reference

Extracted from Claude Code v2.1.19. This is what's available natively in Claude Code
without any external tools. Read references/claude-code-tmux-commands.md for the
specific tmux commands the spawn backend runs through psmux.

## Primitives

| Primitive | What | Where |
|-----------|------|-------|
| Team | Named group of agents. One leader, N teammates. | `~/.claude/teams/{name}/config.json` |
| Teammate | Agent that joined a team. Has name, inbox. | Listed in team config |
| Leader | Agent that created the team. You. | First member in config |
| Task | Work item with status, owner, dependencies. | `~/.claude/tasks/{team}/N.json` |
| Inbox | JSON file for receiving messages. | `~/.claude/teams/{name}/inboxes/{agent}.json` |

## Lifecycle

```
Create Team → Create Tasks → Spawn Teammates → Work → Coordinate → Shutdown → Cleanup
```

## Team Operations

```
# Create team (you become leader)
Teammate({ operation: "spawnTeam", team_name: "my-project" })

# Spawn teammate into team
Task({
  team_name: "my-project",
  name: "worker-1",
  subagent_type: "general-purpose",
  prompt: "Your instructions here",
  run_in_background: true
})

# Message one teammate
Teammate({ operation: "write", target_agent_id: "worker-1", value: "Do X next" })

# Message all teammates (expensive — N messages for N teammates)
Teammate({ operation: "broadcast", name: "leader", value: "Status check" })

# Request teammate shutdown
Teammate({ operation: "requestShutdown", target_agent_id: "worker-1", reason: "Done" })

# Cleanup team resources (all teammates must be shut down first)
Teammate({ operation: "cleanup" })
```

## Task Operations

```
# Create task
TaskCreate({ subject: "Implement auth", description: "...", activeForm: "Implementing..." })

# List all tasks
TaskList()

# Get task details
TaskGet({ taskId: "1" })

# Update task (claim, start, complete, add dependencies)
TaskUpdate({ taskId: "1", owner: "worker-1" })
TaskUpdate({ taskId: "1", status: "in_progress" })
TaskUpdate({ taskId: "1", status: "completed" })
TaskUpdate({ taskId: "2", addBlockedBy: ["1"] })
```

Task statuses: `pending` → `in_progress` → `completed`
When a blocking task completes, blocked tasks auto-unblock.

## Agent Types

| Type | Tools | Best For |
|------|-------|----------|
| `Bash` | Bash only | Git ops, commands |
| `Explore` | Read-only | Codebase scanning (uses Haiku — fast & cheap) |
| `Plan` | Read-only | Architecture design |
| `general-purpose` | All tools | Implementation, multi-step tasks |

## Spawn Backends

Auto-detected based on environment:

| Backend | Trigger | Visibility | Persistence |
|---------|---------|------------|-------------|
| `in-process` | No tmux, no iTerm2 | Hidden | Dies with leader |
| `tmux` | `$TMUX` set or `which tmux` | Visible panes | Survives leader exit |
| `iterm2` | `$TERM_PROGRAM == iTerm.app` + `it2` CLI | Visible panes | Dies with window |

**psmux provides the tmux backend on Windows** — it sets `$TMUX` (to be verified)
and aliases as `tmux`, so Claude Code's auto-detection should pick it up.

## Message Types in Inbox

- Regular text message (from teammate `write`)
- `shutdown_request` / `shutdown_approved` (lifecycle)
- `idle_notification` (teammate finished and is waiting)
- `task_completed` (with task ID and subject)
- `plan_approval_request` (for plan-mode agents)
- `permission_request` (for sandbox/tool permissions)

## Orchestration Patterns

1. **Parallel Specialists** — spawn N reviewers, each checks different aspect, synthesize results
2. **Pipeline** — tasks with sequential dependencies, auto-unblock as each completes
3. **Self-Organizing Swarm** — pool of independent tasks, workers race to claim them
4. **Research → Implement** — synchronous research first, then implementation with results
5. **Plan Approval** — architect proposes, leader approves/rejects before implementation
6. **Coordinated Refactoring** — parallel work on different files, specs depend on both completing
