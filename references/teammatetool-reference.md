# TeammateTool + Task System — Quick Reference

Reflects Claude Code 2.1.178+ (implicit-team model). This is what's available
natively in Claude Code without any external tools. Read
references/claude-code-tmux-commands.md for the specific tmux commands the spawn
backend runs through psmux.

> **Model change (CC 2.1.178):** the `TeamCreate` and `TeamDelete` tools were
> removed. With `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` the session already has
> one implicit team — spawn teammates directly by passing a `name` to the `Agent`
> tool, and coordinate with `SendMessage`. The old `team_name` parameter is
> accepted but ignored.

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
Create Tasks → Spawn Teammates → Work → Coordinate → Shutdown
```

(No team-create or team-cleanup steps — the implicit team exists for the session's
lifetime and goes away with it.)

## Team Operations

```
# Spawn a teammate (passing `name` makes it a visible pane via the tmux backend)
Agent({
  name: "worker-1",
  subagent_type: "general-purpose",
  prompt: "Your instructions here",
  run_in_background: true
})

# Message one teammate
SendMessage({ to: "worker-1", message: "Do X next", summary: "Next step" })

# Request teammate shutdown
SendMessage({ to: "worker-1", message: { type: "shutdown_request" } })
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
