# Agent Orchestration Competitive Landscape (March 2026)

## Key Finding: The Full-Stack Orchestration Gap

NO existing system combines: backlog watching + task decomposition + multi-agent + multi-provider + local execution + health monitoring + result delivery. This is the biggest white space.

## Major Players

| System | Type | Multi-Agent | Multi-Provider | Backlog Watch | Local | Windows | OSS |
|---|---|---|---|---|---|---|---|
| Devin | Cloud autonomous | No | No (proprietary) | Slack trigger | No | No | No |
| Factory AI | Cloud enterprise | Yes (Droids) | No (proprietary) | Jira/Linear/GH | No | No | No |
| OpenAI Codex/Symphony | Cloud autonomous | Yes | No (OpenAI only) | GH Issues | No | No | No |
| Sweep AI | GH bot | No | No (GPT-4) | GH Issues (label) | No | No | Yes (MIT) |
| SWE-agent | Research | No | Yes (LiteLLM) | No | Yes (Docker) | No | Yes (MIT) |
| OpenHands | Platform | No | Yes (LiteLLM) | No | Yes (Docker) | No | Yes (MIT) |
| Aider | Pair programming | Dual-model | Yes (50+) | No | Yes | Yes (works) | Yes (Apache) |
| GH Copilot Workspace | Cloud | No | No (GH/OpenAI) | GH Issues | No | No | No |
| Claude Code TeammateTool | Local multi-agent | Yes | No (Claude only) | No | Yes | Via psmux | No (bundled) |
| dmux | tmux agent spawner | Yes (parallel) | Yes (any CLI) | No | Yes | No (Linux/Mac) | Yes |
| **psmux stack** | **Local multi-agent** | **Yes (Claude+Pi)** | **Yes** | **No (planned)** | **Yes** | **Yes (native)** | **Yes** |

## White Spaces (What Nobody Does Well)

### 1. Provider Arbitrage — Gap: HIGH
No system routes tasks to different LLM providers by cost/complexity/rate limits. Aider's architect/editor is closest but static. psmux architecture describes dynamic routing (Pi for easy, Claude for hard) — unique in the space.

### 2. Windows-Native Agent Orchestration — Gap: HIGH
Zero orchestration platforms are Windows-native. Every multi-agent system assumes Linux/macOS or cloud. psmux is the only project here.

### 3. Full Loop (Backlog + Multi-Agent + Local) — Gap: HIGH
- Factory AI: backlog + multi-agent, but cloud-only
- Sweep: backlog, but single-agent + cloud
- TeammateTool: multi-agent + local, but no backlog watching
- No system combines all three locally.

### 4. Mixed Agent Type Orchestration — Gap: MEDIUM-HIGH
No system mixes different agent runtimes (Claude + Pi + browser agent). All are monolithic single-agent-type. psmux architecture is unique here.

### 5. Health Monitoring for Agent Swarms — Gap: MEDIUM-HIGH
No system has robust heartbeat/liveness, auto-restart, task reassignment, timeout/loop detection for local agent swarms.

### 6. Declarative Pipeline Definitions — Gap: MEDIUM
No system lets you define orchestration as code (YAML/config) — "watch these issues, route this way, spawn these agents."

### 7. Cost Tracking / Budget Enforcement — Gap: MEDIUM
No system tracks aggregate agent swarm cost or enforces budgets.

## psmux Unique Position

The ONLY system that is:
1. Windows-native terminal multiplexer as agent runtime
2. Multi-model agent mixing (Claude Code + Pi)
3. Complexity-based provider routing
4. Uses native Claude Code TeammateTool primitives
5. Git worktree isolation per agent (no Docker)

## Notable Systems for Inspiration

- **Aider**: Architect/editor dual-model pattern = simple provider arbitrage
- **Factory AI**: Specialized "Droids" per task type = agent routing model
- **dmux**: tmux-based agent spawner (Linux/Mac counterpart to psmux)
- **E2B / Modal**: Sandboxed execution infrastructure for AI agents (runtime layer)
- **Sweep AI**: Label-triggered issue→PR automation = backlog watching pattern
