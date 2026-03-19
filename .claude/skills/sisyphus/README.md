# Sisyphus — Iterative Development Loop Plugin

A cross-platform Claude Code plugin that implements autonomous, iterative agent development
by intercepting session exit and re-injecting the original prompt, creating a self-referential
feedback loop. Named after the myth of relentless iteration — keeps pushing until the task
is truly done.

## Overview

```
You run ONCE:
  /sisyphus "Build feature X" --max-iterations 20 --completion-promise "DONE"

Claude Code automatically:
  1. Works on the task
  2. Tries to exit
  3. Stop hook blocks exit
  4. Same prompt fed back with iteration context
  5. Repeat until exit conditions met
```

The loop runs entirely within the current session — no external bash wrappers needed.

## Architecture

```
sisyphus/
├── .claude-plugin/
│   └── plugin.json              # Plugin manifest
├── commands/
│   ├── sisyphus.md              # /sisyphus slash command
│   └── cancel-sisyphus.md       # /cancel-sisyphus slash command
├── hooks/
│   ├── hooks.json               # Hook registration (Stop hook)
│   └── stop_hook.py             # Core loop mechanism (cross-platform Python)
├── scripts/
│   └── setup_sisyphus.py        # Argument parsing & state file creation
├── SKILL.md                     # Skill metadata
├── PRD.md                       # Product Requirements Document
└── README.md                    # This file
```

## Usage

### Basic loop with iteration limit
```
/sisyphus Build a REST API for todos with CRUD and tests --max-iterations 20
```

### Loop with completion promise
```
/sisyphus Implement user auth with JWT. When all tests pass,
  output <promise>AUTH COMPLETE</promise>
  --completion-promise "AUTH COMPLETE" --max-iterations 15
```

### TDD cycle
```
/sisyphus Implement feature X following TDD:
  1. Write failing tests first
  2. Implement minimum code to pass
  3. Run tests — if any fail, debug and fix
  4. Repeat until all green
  Output <promise>ALL TESTS GREEN</promise>
  --completion-promise "ALL TESTS GREEN" --max-iterations 20
```

### Cancel a loop
```
/cancel-sisyphus
```

### Monitor progress
```bash
cat .claude/sisyphus.local.md
```

## Exit Conditions

| Condition | Mechanism |
|-----------|-----------|
| Max iterations | `iteration >= max_iterations` |
| Completion promise | `<promise>TEXT</promise>` in output (exact match) |
| Manual cancel | `/cancel-sisyphus` removes state file |
| State file deletion | `rm .claude/sisyphus.local.md` |

## Cross-Platform

All scripts are written in Python (stdlib only, no dependencies). Works on:
- Windows (via `py` or `python3`)
- macOS / Linux (via `python3`)

## State File

`.claude/sisyphus.local.md` — YAML frontmatter + prompt body, human-readable:

```yaml
---
active: true
iteration: 3
max_iterations: 20
completion_promise: "COMPLETE"
started_at: "2026-02-14T10:30:00Z"
---

Build a REST API for todos. Output <promise>COMPLETE</promise> when done.
```

## License

MIT
