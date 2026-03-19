# Architecture: psmux as a Claude Code Swarm Backend

## The Stack

```
┌─────────────────────────────────────────────────────────────────┐
│  YOU (one prompt: "implement OAuth, add tests, review")         │
└──────────────────────────┬──────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│  LEADER (Claude Code session)                                   │
│                                                                 │
│  Reads: CLAUDE.md + swarm-orchestrator skill                    │
│  Does:  Decomposes work → creates Tasks → spawns agents         │
│         Monitors inboxes → reassigns failures → merges results  │
│                                                                 │
│  Uses:  TeammateTool (teams, inboxes, tasks)                    │
│         psmux (split-window, send-keys, list-panes)             │
│         git worktree (isolation per agent)                       │
└──────┬──────────┬──────────┬──────────┬─────────────────────────┘
       │          │          │          │
       ▼          ▼          ▼          ▼
┌──────────┐┌──────────┐┌──────────┐┌──────────┐
│ psmux %1 ││ psmux %2 ││ psmux %3 ││ psmux %4 │
│          ││          ││          ││          │
│ claude   ││ claude   ││ pi       ││ claude   │
│ (builder)││ (builder)││ (builder)││(reviewer)│
│          ││          ││          ││          │
│ worktree ││ worktree ││ worktree ││ main     │
│ /wt/auth ││ /wt/api  ││ /wt/test ││ branch   │
└──────────┘└──────────┘└──────────┘└──────────┘
     │            │            │           │
     └────────────┴────────────┴───────────┘
                       │
                       ▼
              ┌─────────────────┐
              │  git merge      │
              │  (reviewer PRs) │
              └─────────────────┘
```

## Component Responsibilities

### psmux (Terminal Layer)
- Provides visible, isolated terminal panes for each agent
- Session persistence — agents survive if you disconnect
- `send-keys` for injecting prompts into agent panes
- `list-panes` for monitoring which agents are alive
- `kill-pane` for cleaning up finished/failed agents
- `capture-pane` for reading agent output programmatically

psmux replaces tmux on Windows. Claude Code's TeammateTool spawn backend
calls tmux commands — since psmux aliases as `tmux`, it's a drop-in.

### TeammateTool (Coordination Layer)
Built into Claude Code natively. Provides:
- **Teams**: named groups of agents with a leader
- **Inboxes**: JSON files for agent-to-agent messaging
- **Tasks**: work items with status, ownership, and dependencies
- **Lifecycle**: spawn, message, shutdown, cleanup

This is what replaced the need for Overstory. Claude Code ships with
the same coordination primitives that Overstory built externally.

### Git Worktrees (Isolation Layer)
Each agent works in its own worktree — a separate checkout of the same
repo on a separate branch. This means:
- No merge conflicts during parallel work
- Each agent sees only its own changes
- Merging happens after review, not during implementation

```
/workspace/
├── .git/                    # shared git objects
├── src/                     # main branch (reviewer works here)
└── .worktrees/
    ├── auth-feature/        # agent 1's branch
    ├── api-endpoints/       # agent 2's branch
    └── test-coverage/       # agent 3's branch
```

### Skills (Knowledge Layer)
Loaded on-demand based on conversation context:
- **psmux-project**: coding conventions, test patterns, release workflow
- **swarm-orchestrator**: how the leader decomposes and delegates work
- **swarm-backend-validation**: testing psmux as a tmux backend
- **pi-dispatch**: bridging Pi agents into the TeammateTool inbox system

### Agents (Specialist Definitions)
Subagent configs that define tool access and behavior:
- **psmux-builder**: full tool access, loads `psmux-project` skill
- **psmux-reviewer**: read-only + review skills, audits diffs
- **psmux-explorer**: read-only, fast codebase scanning with Haiku

## Data Flow

1. **You** → give leader a high-level task
2. **Leader** → reads CLAUDE.md, loads swarm-orchestrator skill
3. **Leader** → creates git worktrees for each subtask
4. **Leader** → calls `psmux split-window` to create agent panes
5. **Leader** → calls `psmux send-keys` to inject `claude -p` or `pi -p` into each pane
6. **Agents** → work independently in their worktrees
7. **Agents** → write results to TeammateTool inbox files when done
8. **Leader** → reads inbox, checks task status
9. **Leader** → sends completed worktrees to reviewer pane
10. **Reviewer** → checks diff, approves or creates new task for fixes
11. **Leader** → merges approved worktrees into main branch
12. **Leader** → runs full test suite on merged result
13. **Leader** → if tests fail, creates new tasks and loops

## Why Mix Claude Code and Pi?

Claude Code: heavier, smarter, more tools, better at architecture and review.
Pi coding agent: lighter, faster, 4 tools, cheaper per task.

The leader routes based on task complexity:
- "Implement OAuth2 with token refresh" → Claude Code (complex, multi-file)
- "Add unit test for parse_args()" → Pi (focused, single-file)
- "Fix clippy warning in line 42" → Pi (trivial, surgical)
- "Review the auth module for safety" → Claude Code (judgment-heavy)

This maximizes throughput: Pi handles the volume, Claude handles the complexity.

## Rate Limit Reality

Concurrent API calls are the bottleneck, not psmux:
- Claude Max subscription: ~3-5 concurrent Claude Code sessions before throttling
- Pi with Anthropic API: shares the same rate limit pool
- Pi with other providers (Ollama local, OpenRouter): separate limits

Practical ceiling: 3 Claude Code agents + 2 Pi agents, or adjust mix based on
which provider each Pi agent uses. The leader should stagger spawns if hitting limits.

## Comparison With Alternatives

| | This Stack | Overstory | dmux | Raw tmux scripts |
|---|---|---|---|---|
| Windows native | ✅ (psmux) | ❌ (needs WSL) | ❌ (Linux/Mac) | ❌ |
| Multi-model agents | ✅ (Claude + Pi) | ❌ (Claude only) | ✅ (any agent) | ✅ (manual) |
| Native Claude Code integration | ✅ (TeammateTool) | ❌ (custom wrapper) | ❌ (separate tool) | ❌ |
| Task dependencies | ✅ (TaskCreate/Update) | ✅ (internal) | ❌ | ❌ |
| Agent communication | ✅ (inboxes) | ✅ (internal) | ❌ | ❌ |
| Project-specific skills | ✅ (skills/) | ❌ | ❌ | ❌ |
| Monitoring UI | ⚠️ (psmux panes + JSON) | ✅ (dashboard) | ✅ (tmux panes) | ⚠️ (tmux panes) |
