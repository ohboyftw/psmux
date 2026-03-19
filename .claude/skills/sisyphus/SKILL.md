---
name: sisyphus
description: >
  Iterative self-referential development loop plugin for Claude Code.
  Uses stop hook to intercept session exit and re-inject prompts for
  progressive refinement. Use for TDD cycles, multi-phase builds,
  and autonomous iterative development. Invoke with /sisyphus command.
  Cancel with /cancel-sisyphus. Cross-platform (Windows + Unix).
---

# Sisyphus — Iterative Development Loop

Claude works on a task, attempts to exit, gets intercepted by the stop hook,
and receives the same prompt again to iterate on its prior work visible in
files and git history. Keeps pushing until the task is truly done.

## Commands

| Command | Purpose |
|---------|---------|
| `/sisyphus PROMPT [--max-iterations N] [--completion-promise TEXT]` | Start a loop |
| `/cancel-sisyphus` | Emergency stop |

## Architecture

```
/sisyphus "task" --max-iterations 20
    |
    v
setup_sisyphus.py creates .claude/sisyphus.local.md
    |
    v
+---------------------------------+
| Claude works on task (iter N)   |<---------+
| Reads files, git log, tests     |          |
+----------------+----------------+          |
                 | tries to exit             |
                 v                           |
+---------------------------------+          |
| stop_hook.py intercepts exit    |          |
| - Checks max iterations         |          |
| - Checks completion promise     |          |
| - Increments counter            |----------+
| - Re-injects prompt             |
+----------------+----------------+
                 | exit conditions met
                 v
          Session ends normally
```

## Exit Conditions

| Condition | Mechanism |
|-----------|-----------|
| Max iterations | `iteration >= max_iterations` |
| Completion promise | `<promise>TEXT</promise>` in output (exact match) |
| Manual cancel | `/cancel-sisyphus` removes state file |

## State File

`.claude/sisyphus.local.md` — YAML frontmatter + prompt body. Human-readable.

```yaml
---
active: true
iteration: 3
max_iterations: 20
completion_promise: "ALL TESTS GREEN"
started_at: "2026-02-14T10:30:00Z"
---

Build feature X with TDD...
```
