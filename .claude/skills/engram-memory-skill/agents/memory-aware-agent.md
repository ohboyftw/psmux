---
title: "Memory-Aware Agent"
description: >
  Instructions for Claude Code agents on how to use Engram memory
  alongside Serena documentation for optimal context assembly.
---

# Memory-Aware Agent Protocol

## When to Use What

| Need | System | Command |
|------|--------|---------|
| "What's documented about X?" | **Serena** | `serena search . "X"` |
| "What do I know about X?" | **Engram** | `engram recall . "X"` |
| "Everything about X" | **Both** | `engram recall . "X" --bridge` |
| "Remember that we decided X" | **Engram** | `engram remember . --type decision "X"` |
| "Update the docs about X" | **Serena** | Update docs/, run gardener |
| "How is X related to Y?" | **Engram** | `engram relate . "X"` |
| "Why did we decide X?" | **Engram** | `engram recall . "X" --type decision` |

## Session Protocol

### Starting a Session

1. **Read AGENTS.md** (Serena — what exists, where to look)
2. **Recall task context** (Engram — what was learned about this area)
   ```bash
   python engram_cli.py recall . "<task description>" --bridge
   ```
3. Proceed with work, informed by both static docs and dynamic memory

### During Work

- When you **make a decision**: store it
  ```bash
  python engram_cli.py remember . --type decision "Chose X over Y because Z"
  ```
- When you **discover a pattern**: store it
  ```bash
  python engram_cli.py remember . --type pattern "This codebase uses X pattern for Y"
  ```
- When you **fix a bug** through multi-step reasoning: store the trace
  ```bash
  python engram_cli.py trace . "Fix timeout on /api/auth" --steps "Step 1" "Step 2" --outcome success
  ```

### Ending a Session

1. Store any unstored decisions or discoveries
2. Update Serena docs if code changed significantly
3. The next session will benefit from what this session learned

## What NOT to Store

- Obvious facts (the codebase uses Python — that's in the docs)
- Temporary debugging state (unless the fix is reusable)
- Personal opinions without technical rationale
- Anything already well-documented in Serena's docs/

## What TO Store

- **Decisions with rationale** — "chose X because Y, considered Z"
- **Surprising discoveries** — "the auth module actually calls service A, not B"
- **Error patterns** — "this type of error is always caused by..."
- **Successful approaches** — "for this kind of task, first do A then B"
- **User preferences** — "prefers explicit error handling over try/except"
- **Relationship context** — "module A depends on B because of C"
