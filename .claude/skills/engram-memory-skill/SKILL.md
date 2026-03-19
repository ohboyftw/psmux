---
name: engram-memory
description: >
  DEPRECATED: Engram is now available as an MCP server with native tools.
  Do NOT use this skill. Instead use the engram MCP tools directly:
  engram_remember, engram_recall, engram_relate, engram_trace, engram_stats.
  The MCP server is configured in ~/.claude/settings.json.
enabled: false
---

# Engram Memory — Now via MCP Server

This skill has been superseded by the Engram MCP server (2026-02-13).

Instead of subprocess CLI calls, use the native MCP tools:

| MCP Tool | What it does |
|----------|-------------|
| `engram_remember` | Store a memory (fact, decision, pattern, trace) |
| `engram_recall` | Search memory, optionally with Serena bridge |
| `engram_relate` | Explore entity relationships |
| `engram_trace` | Store a reasoning trace |
| `engram_stats` | View memory statistics |

## Why MCP?

- Cold start paid once when server launches (~3-5s)
- Each tool call: ~50-100ms (vs ~1-1.5s per CLI subprocess)
- Native Claude Code tool integration (no bash needed)

## Fallback CLI

The CLI still works if needed:
```bash
py ~/.claude/skills/engram-memory-skill/engram_cli.py recall /path/to/project "query"
```

## Full docs

See `SKILL.md.original` for the complete architecture documentation.
