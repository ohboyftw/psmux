---
name: llm-council
description: >
  Multi-LLM code review ensemble (Claude, GPT-5.3, Gemini, MiniMax M1, Kimi K2.5)
  + CodeRabbit. Now runs as an MCP server. Use the native MCP tools:
  council_review, council_benchmark, council_status.
  The MCP server is configured in ~/.claude/settings.json.
---

# LLM Council — MCP Server

5-model code review ensemble that fans out to all configured LLM providers in parallel,
synthesizes their consensus, then merges with CodeRabbit for a grand synthesis report.

## MCP Tools

| Tool | Description |
|------|-------------|
| `council_review` | Run multi-LLM review on a diff. Returns consensus report. |
| `council_benchmark` | Score all sources against ground truth test cases. |
| `council_status` | Show which API keys are configured and active providers. |

## Architecture

```
Diff --> Claude, GPT-5.3, Gemini, MiniMax, Kimi (parallel)
           |
           v
     Council Synthesis (LLM consensus)
           |
           +--- CodeRabbit (independent) ---+
           |                                |
           v                                v
                   Grand Synthesis
                        |
                     Report
```

## 8 Independently Scorable Layers

1. claude  2. gpt-53  3. gemini  4. minimax  5. kimi-k25
6. coderabbit  7. council-synthesis  8. grand-synthesis

## API Keys (set what you have — auto-detects)

| Variable | Provider |
|----------|----------|
| `ANTHROPIC_API_KEY` | Claude (also synthesizer) |
| `OPENAI_API_KEY` | GPT-5.3 |
| `GOOGLE_API_KEY` | Gemini |
| `MINIMAX_API_KEY` | MiniMax M1 (api.minimax.io) |
| `MOONSHOT_API_KEY` | Kimi K2.5 (api.moonshot.ai) |
| `CODERABBIT_API_KEY` | CodeRabbit |
| `OPENROUTER_API_KEY` | Universal fallback |

## MCP Server Registration

In `~/.claude/settings.json`:
```json
"llm-council": {
  "command": "py",
  "args": ["C:\\Users\\aravi\\.claude\\skills\\llm-council\\mcp_server.py"]
}
```
