---
name: rlm-workspace
description: >
  DEPRECATED as a skill. RLM Workspace is now an MCP server.
  Use the native MCP tools (rlm_load, rlm_exec, rlm_vars, rlm_analyze, rlm_status)
  instead of the old CLI. The MCP server is configured in ~/.claude/settings.json.
---

# RLM Workspace — MCP Server

> **This skill is deprecated.** RLM Workspace v2.0 runs as an MCP server with native tools.
> The old CLI (`rlm_cli.py`) and bridge (`bridge.py`) have been removed.

## MCP Tools

| Tool | Mode | Description |
|------|------|-------------|
| `rlm_load` | Agent-driven | Load file(s) into workspace as a Python variable |
| `rlm_exec` | Agent-driven | Execute Python code in the sandboxed REPL |
| `rlm_vars` | Agent-driven | List loaded variables with types and sizes |
| `rlm_analyze` | Autonomous | Run full RLM analysis loop (requires `rlms` package) |
| `rlm_status` | Management | Show workspace state, config, and feature availability |

## Two-Mode Architecture

### Agent-Driven Mode (recommended)

Claude Code loads content into the workspace and writes Python exploration code directly.
No external LLM needed — Claude IS the reasoning engine.

```
1. rlm_load("src/", recursive=True, glob_pattern="*.py")
2. rlm_exec("lines = context.split('\\n'); print(len(lines))")
3. rlm_exec("import re; classes = re.findall(r'class (\\w+)', context); print(classes)")
4. Read results, reason, write more code...
```

### Autonomous Mode (optional)

The `rlm` library runs its own iterative REPL loop with a local LLM (Ollama).
Fire-and-forget: call `rlm_analyze` and get back a complete analysis.

Requires: `pip install rlms`

## Configuration

Per-project config at `.rlm/config.yaml`. Falls back to Ollama defaults.

```yaml
backend: openai
backend_kwargs:
  model_name: qwen3:8b
  base_url: http://localhost:11434/v1
  api_key: ollama
repl_timeout: 30
```

## MCP Server Registration

In `~/.claude/settings.json`:
```json
"rlm": {
  "command": "py",
  "args": ["C:\\Users\\aravi\\.claude\\skills\\rlm-workspace\\mcp_server.py"]
}
```
