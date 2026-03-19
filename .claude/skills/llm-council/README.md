# LLM Council

**Multi-LLM code review ensemble** that fans out diffs to 5+ language models in parallel, synthesizes their consensus, then optionally merges with CodeRabbit for a final verdict. Ships with a benchmarking harness that scores every review source independently against ground truth.

## Why?

No single LLM catches everything. Claude is strong on architecture, GPT excels at edge cases, Gemini spots performance issues, and CodeRabbit understands AST structure. LLM Council runs them all in parallel (~3s wall time), then synthesizes a consensus that outperforms any individual reviewer.

## Architecture

```
                         ┌──────────────────────────────────────┐
                         │            Diff Input                │
                         └──┬────┬────┬────┬────┬────┬─────────┘
                            │    │    │    │    │    │
                            v    v    v    v    v    v
                         Claude GPT  Gem  Mini Kimi CodeRabbit
                         Sonnet 5.2  ini  Max  K2.5   (AST)
                            │    │    │    │    │    │
                            └─┬──┴──┬─┴────┘    │    │
                              │     │           │    │
                         Council Synthesis      │    │
                           (LLM consensus)      │    │
                              │                 │    │
                              └────────┬────────┘    │
                                       │             │
                                  Grand Synthesis <──┘
                                  (council + CR)
                                       │
                                    Report
```

**8 independently scorable layers**: each individual model, CodeRabbit, council synthesis, and grand synthesis — all benchmarked separately.

## Quick Start

### Install

```bash
pip install -r requirements.txt

# Or with optional native SDKs for better performance:
pip install llm-council[all-providers]
```

### Set API Keys

```bash
# At least 2 for council consensus
export ANTHROPIC_API_KEY="..."
export OPENAI_API_KEY="..."
export GOOGLE_API_KEY="..."
export MINIMAX_API_KEY="..."      # api.minimax.io
export MOONSHOT_API_KEY="..."     # api.moonshot.ai (Kimi K2.5)
export CODERABBIT_API_KEY="..."   # optional
```

The council **auto-detects** which keys are set and only activates those providers. Minimum 2 models required for consensus synthesis.

### Run a Review

```bash
# Pipe a diff
git diff HEAD | python council.py

# From file
python council.py -d changes.patch

# JSON output
python council.py -d changes.patch --json

# Select specific models
python council.py -d changes.patch --models "anthropic:claude-sonnet-4-20250514" "moonshot:kimi-k2.5"

# Skip CodeRabbit
python council.py -d changes.patch --no-coderabbit
```

### MCP Server (for Claude Code / AI agents)

```bash
python mcp_server.py
```

Register in your MCP client config:

```json
{
  "mcpServers": {
    "llm-council": {
      "command": "python",
      "args": ["/path/to/llm-council/mcp_server.py"]
    }
  }
}
```

**MCP Tools:**

| Tool | Purpose |
|------|---------|
| `council_review` | Run ensemble review on a diff |
| `council_benchmark` | Run evaluation suite against ground truth |
| `council_status` | Show which providers are active |

## Providers

| Provider | Model | API Base | Notes |
|----------|-------|----------|-------|
| Anthropic | Claude Sonnet 4 | api.anthropic.com | Also used as synthesizer |
| OpenAI | GPT-5.2 Codex | api.openai.com | Supports Responses API |
| Google | Gemini 2.0 Flash | googleapis.com | Fast, good at perf issues |
| MiniMax | MiniMax-M2.1 | api.minimax.io/v1 | OpenAI-compatible, 1M context |
| Moonshot | Kimi K2.5 | api.moonshot.ai/v1 | OpenAI-compatible, thinking mode |
| OpenRouter | Any model | openrouter.ai/api/v1 | Universal fallback |
| CodeRabbit | AST analysis | api.coderabbit.ai | API + CLI modes |

All non-Anthropic/OpenAI/Google providers use httpx directly — no SDK required. Native SDKs are optional for better error handling and streaming.

## Benchmarking

The evaluation harness scores every review source against ground truth using precision, recall, and F1 with fuzzy matching.

**5 built-in test cases:**

| Case | Vulnerability | Expected Findings |
|------|--------------|-------------------|
| `sql-injection` | f-string in SQL query | 2 critical |
| `race-condition` | Non-atomic counter | 2 findings |
| `react-memory-leak` | Missing useEffect cleanup | 3 findings |
| `auth-bypass` | Missing @require_admin | 1 critical |
| `n-plus-one` | N+1 query in loop | 2 major |

```bash
# Run full benchmark
python evaluate.py

# Without CodeRabbit
python evaluate.py --no-coderabbit

# Specific cases
python evaluate.py --cases sql-injection auth-bypass

# JSON output for CI
python evaluate.py --json -o results.json
```

**Output includes:**
- Leaderboard ranked by F1 score with 95% confidence intervals
- Per-case recall heatmap
- Head-to-head F1 deltas between sources
- Insights: best overall, best recall, best precision, synthesis value analysis

## Configuration

Edit `config.json` to change models, weights, or endpoints:

```json
{
  "council": {
    "members": [
      {"source_id": "claude", "provider": "anthropic", "model": "claude-sonnet-4-20250514", "weight": 1.0},
      {"source_id": "gpt-53", "provider": "openai", "model": "gpt-5.2-codex", "weight": 0.9}
    ],
    "synthesizer": {
      "provider": "anthropic",
      "model": "claude-sonnet-4-20250514"
    }
  }
}
```

## Project Structure

```
llm-council/
├── council.py              # Core engine — 5 LLM providers + CodeRabbit orchestration
├── evaluate.py             # Benchmarking harness — 5 test cases, 8-layer scoring
├── mcp_server.py           # MCP server interface (3 tools)
├── config.json             # Model configuration and endpoints
├── requirements.txt        # Dependencies (httpx core, SDKs optional)
├── pyproject.toml          # Python packaging
├── scripts/
│   └── capture_coderabbit.py   # CodeRabbit baseline capture utility
└── eval_data/
    └── coderabbit_captures/    # Cached CodeRabbit results for benchmarking
```

## How It Works

1. **Fan-out**: All active LLMs receive the diff + optional context in parallel via `asyncio.gather()`
2. **Parse**: Each response is parsed into structured `Finding` objects (severity, category, file, line, description, suggestion)
3. **Council synthesis**: If 2+ LLMs responded, Claude synthesizes consensus findings — unanimous agreements, majority opinions, unique insights
4. **Grand synthesis**: If CodeRabbit also ran, its AST-level findings are merged with the council consensus
5. **Report**: Markdown or JSON output with all findings, confidence scores, and latency metrics

## Design Decisions

- **httpx as universal transport** — All providers work via httpx. Native SDKs are optional optimizations, not requirements.
- **Async throughout** — Parallel fan-out means wall time is bounded by the slowest model, not the sum.
- **Independent scoring** — Every review source (individual + synthesis) is benchmarked separately. This proves whether synthesis adds value.
- **Graceful degradation** — Missing API keys skip that provider silently. Missing SDKs fall back to httpx. The council works with 2+ models.
- **Deterministic reviews** — Temperature 0.2 for most models (except Kimi at 1.0 for thinking mode).

## License

MIT
