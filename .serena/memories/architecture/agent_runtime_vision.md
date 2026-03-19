# psmux as Windows Agent Runtime — Architectural Vision

Source: Analysis session 2026-03-17 (psmux + cmux + Pi integration deep-dive)

## Core Insight

psmux is evolving beyond a terminal multiplexer into an **agent runtime** — the process substrate on which AI coding agents (Claude Code, Pi, future agents) are spawned, isolated, monitored, and coordinated on Windows.

The multiplexer is the kernel; the orchestration is the OS.

## Reference Systems

| System | What It Does | Relevance to psmux |
|---|---|---|
| **Symphony** (OpenAI) | Monitors issue tracker → spawns coding agents → delivers PRs | psmux is the local-first, multi-provider alternative to this cloud-hosted approach |
| **Conductor** (Netflix OSS) | DAG-based workflow engine with task routing, retries, human-in-loop | psmux + swarm-orchestrator skill is an embryonic version of this |
| **cmux** (manaflow-ai) | macOS terminal emulator with agent notifications, browser, workspaces | Complementary — covers macOS; psmux covers Windows. Shared protocol is the bridge |
| **Overstory** metaphor | "See everything from above" | The swarm-status / dashboard layer psmux needs |
| **Superset** (Apache) | Data exploration/visualization | The observability/reporting layer for swarm runs |

## Layered Architecture

```
Layer 4: Backlog Integration    ← GitHub Issues / Linear MCP
              │                    Polls for `agent-ready` issues
              │                    Decomposes into agent-sized tasks
              │
Layer 3: Task Decomposition     ← Claude Code leader agent
              │                    Analyzes issue complexity
              │                    Routes to Pi (focused) vs Claude (complex)
              │                    Creates git worktrees for isolation
              │
Layer 2: Agent Orchestration    ← swarm-orchestrator + pi-dispatch skills
              │                    TeammateTool inbox coordination
              │                    Provider rotation (Anthropic/Ollama/OpenRouter)
              │                    Rate limit arbitrage across providers
              │
Layer 1: Agent Runtime          ← psmux (THIS IS THE CORE)
              │                    Process lifecycle (spawn/kill/health)
              │                    IPC (TCP + auth + port files)
              │                    Warm session pools (sub-50ms spawn)
              │                    Output capture (capture-pane)
              │                    Namespace isolation (-L flag)
              │                    Agent metadata in pane options
              │                    JSON structured output
              │
Layer 0: OS                     ← Windows 10/11
                                   ConPTY, VT100, localhost TCP
```

## What Exists Today (Building Blocks)

- Process isolation: psmux panes + `-L` namespaces
- Agent spawning: `send-keys`, `split-window`, `-P -F "#{pane_id}"`
- Instant startup: warm session claiming (v3.2.0, ~50ms)
- Output capture: `capture-pane -p`
- Pi integration: `pi-bridge.ps1` (provider routing, inbox bridging)
- Swarm orchestration: `swarm-orchestrator` skill (TeammateTool coordination)
- Git isolation: worktree-per-agent pattern
- Task coordination: TeammateTool JSON inboxes
- 46/47 tmux commands: transparent backend for Claude Code

## What's Missing (The Gap)

### Must Have (for "Conductor" status)
1. **JSON output mode** (`--json` flag) — structured output for all programmatic consumers
2. **Warm pool sizing** (`set -g warm-pool-size N`) — multiple pre-spawned sessions for parallel agent launch
3. **Agent metadata** — queryable key=value pairs per pane for "who is doing what"
4. **`swarm-status` command** — single view of all agents across all sessions/namespaces
5. **Health monitoring** — detect hung/dead agents, notify leader

### Nice to Have (for "Overstory" observability)
6. **OSC notifications** — Windows Toast when agents need attention
7. **Audit logging** — command log for swarm replay/debugging
8. **Backlog watcher** — poll GitHub Issues for `agent-ready` label, auto-dispatch
9. **Result aggregator** — collect PRs/diffs/reports from completed agent tasks

### Future (for "Symphony" equivalence)
10. **MCP server wrapper** — expose psmux as MCP tools for any LLM agent framework
11. **Cross-platform protocol** — shared SwarmBackend trait with cmux for universal orchestration
12. **Provider-aware scheduling** — route tasks based on provider availability/cost/rate limits

## Differentiators vs Symphony/Cloud Orchestration

| Aspect | Symphony (cloud) | psmux (local) |
|---|---|---|
| Hosting | Cloud-hosted, provider-locked | Local-first, runs on developer's machine |
| Providers | Single (OpenAI) | Multi (Anthropic, Ollama, OpenRouter, Gemini via Pi) |
| Latency | Network round-trip per action | Sub-50ms (warm sessions, localhost TCP) |
| Cost | API costs for all agents | Local Ollama = free for bulk work |
| Visibility | Web dashboard | Terminal-native (capture-pane, list-panes) |
| Control | Opaque cloud process | Full process visibility, kill/restart/inspect |
| Platform | Cloud-agnostic | Windows-native (the underserved platform) |

## Multi-Provider Rate Limit Arbitrage

Key competitive advantage: Pi agents can use different LLM providers to avoid competing for the same API quota:

```
psmux session "swarm"
├── Pane %0: Claude Code (Anthropic) — architecture task
├── Pane %1: Pi (Anthropic) — impl task A
├── Pane %2: Pi (Ollama/local) — impl task B ← free, no rate limit
├── Pane %3: Pi (OpenRouter) — impl task C ← separate quota
└── Pane %4: Pi (Gemini) — test writing ← separate quota
```

Result: 4-5x more parallel agents than single-provider swarms.

## Implementation Roadmap

**Phase 1 — Foundation (P0 tasks)**
Ship JSON output mode + harden Pi bridge. These unblock everything above.

**Phase 2 — Runtime (P1 tasks)**
Warm pool config + agent metadata + swarm-status command. This makes psmux a proper agent runtime.

**Phase 3 — Orchestration (P2 tasks)**
Health monitoring + backlog watcher + result aggregation. This reaches Conductor-level orchestration.

**Phase 4 — Platform (P3-P4 tasks)**
MCP server wrapper + cross-platform protocol + provider scheduling. This reaches Symphony-level capability, but local-first and multi-provider.
