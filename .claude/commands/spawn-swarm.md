---
description: Launch a multi-agent swarm to work on a task in parallel
allowed-tools: Bash, Read, Write, Edit, Task, Teammate, TaskCreate, TaskUpdate, TaskList
---

Launch a multi-agent swarm to accomplish this: $ARGUMENTS

Follow the swarm-orchestrator skill workflow:

1. **Decompose** the request into 2-5 independent subtasks. Each should be
   scoped to specific files/modules to avoid merge conflicts.

2. **Create worktrees** — one per subtask:
   ```
   git worktree add .worktrees/<task-name> -b agent/<task-name>
   ```

3. **Create team and tasks** using TeammateTool:
   - `spawnTeam` with a descriptive name
   - `TaskCreate` for each subtask
   - `TaskCreate` for a review task that depends on all implementation tasks
   - `TaskUpdate` to set dependencies

4. **Route agents** — decide Claude Code vs Pi for each task based on complexity:
   - Complex/multi-file → Claude Code (general-purpose)
   - Simple/focused → Pi (via Bash subagent + pi-bridge.ps1)
   - Exploration → Claude Code (Explore with Haiku)

5. **Spawn agents** — stagger to avoid rate limits (2-3 at a time)

6. **Monitor** — periodically check inbox and task status

7. **Review and merge** when all tasks complete

8. **Run full CI** — `cargo fmt --check && cargo clippy -- -D warnings && cargo test`

9. **Handle failures** — create new tasks for any test failures, spawn fix agents

10. **Cleanup** — shutdown all teammates, remove worktrees, cleanup team

Report progress at each phase. If rate-limited, pause and wait before spawning more agents.
