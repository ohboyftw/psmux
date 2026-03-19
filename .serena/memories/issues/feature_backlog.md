# psmux Feature Backlog — Prioritized

Source: Analysis session 2026-03-17 (psmux + cmux + Pi integration analysis)

## P0 — High Impact, Low Effort (Do First)

### 1. JSON Output Mode (`--json`)
**What**: Add `--json` flag to `list-panes`, `list-sessions`, `display-message`, `capture-pane` etc. for structured output.
**Why**: Unblocks everything else — MCP wrappers, Pi bridge, monitoring tools all need structured output instead of parsing tmux text format.
**Effort**: Small — serde_json already a dependency, add serialization to existing response paths.
**Files**: `src/client.rs`, `src/format.rs`, `src/server/connection.rs`

### 2. Harden pi-bridge.ps1
**What**: Improve the Pi coding agent bridge script with provider rotation (Ollama fallback when Anthropic rate-limited), warm session claiming integration, and structured JSON result reporting.
**Why**: Immediate swarm throughput gain — Pi agents can use local Ollama or OpenRouter to avoid competing for Anthropic API quota.
**Effort**: Small — script already exists at `.claude/skills/pi-dispatch/scripts/pi-bridge.ps1`.
**Files**: `.claude/skills/pi-dispatch/`

## P1 — High Impact, Medium Effort

### 3. Warm Pool Size Configuration
**What**: `set -g warm-pool-size N` to pre-spawn N warm sessions instead of just 1.
**Why**: Swarms spawn multiple agents simultaneously — having 3-5 warm sessions ready means all agents start in ~50ms instead of only the first one.
**Effort**: Small — extend existing warm session logic in `src/server/mod.rs`.
**Files**: `src/server/mod.rs`, `src/server/options.rs`, `src/session.rs`

### 4. Agent Metadata in Pane Options
**What**: `select-pane -P "agent=claude-code-1,task=auth-oauth,provider=anthropic"` stored and queryable via `list-panes -F "#{pane_agent}"`.
**Why**: Key for swarm observability — leader needs to query "which pane is doing what task with which provider".
**Effort**: Medium — extend `Pane` struct, add format string expansion, parse key=value from pane style.
**Files**: `src/pane.rs`, `src/format.rs`, `src/types.rs`

### 5. Warm Pi Pool
**What**: Pre-spawn Pi agents in warm psmux sessions with local Ollama, ready for instant task dispatch.
**Why**: Combines #3 + #2 — ~250ms total agent spawn time (50ms warm claim + 200ms Pi init).
**Effort**: Small (once #2 and #3 are done) — orchestration script/skill.
**Files**: `.claude/skills/swarm-orchestrator/`, `.claude/skills/pi-dispatch/`

## P2 — Medium Impact

### 6. List All Panes Across Sessions
**What**: `list-all-panes` or `list-panes -a` command to query all panes across all sessions in a namespace.
**Why**: One command for full swarm status instead of looping per-session.
**Effort**: Small — iterate session port files, connect to each, aggregate results.
**Files**: `src/main.rs`, `src/session.rs`

### 7. OSC Notification Support
**What**: Parse OSC 9/99/777 escape sequences from agent panes, surface as Windows Toast notifications.
**Why**: Mirrors cmux's blue-ring UX — agents can signal "need attention" without polling.
**Effort**: Medium — VT100 parser extension + Windows notification API.
**Files**: `crates/vt100-psmux/`, `src/platform.rs`

### 8. Agent Health Check
**What**: Detect hung panes via `#{pane_pid}` process liveness check, auto-notify leader when an agent dies or hangs.
**Why**: Long-running swarms need to detect stuck agents to avoid wasted time/resources.
**Effort**: Medium — periodic process check + hook trigger.
**Files**: `src/server/mod.rs`, `src/pane.rs`

### 9. MCP Server Wrapper for psmux
**What**: Expose psmux as MCP tools (`list_surfaces`, `send_input`, `read_screen`, `create_session`, etc.) similar to cmux's `cmuxlayer`.
**Why**: Makes psmux a first-class MCP tool for any LLM agent, not just Claude Code's TeammateTool.
**Effort**: Medium — separate Python/TypeScript MCP server calling psmux CLI.
**Files**: New project or `tools/` directory

## P3 — Lower Priority

### 10. Pi Extension ↔ psmux Hook Coordination
**What**: `pane-died` hook triggers inbox notification; Pi extension signals `TASK_COMPLETE` via `send-keys`.
**Why**: Elegant event-driven coordination, though Pi bridge covers most of this already.
**Effort**: Small.

### 11. Provider-Aware Pane Environment
**What**: Inject `$PI_PROVIDER`, `$ANTHROPIC_API_KEY` etc. per-pane so agents auto-use assigned provider.
**Why**: Convenience for multi-provider swarms — currently done manually in `send-keys`.
**Effort**: Small.

### 12. Audit Logging
**What**: Log all server commands to file for swarm replay/debugging.
**Why**: Debug tool for post-mortem analysis of swarm runs.
**Effort**: Small.
**Files**: `src/server/connection.rs`

### 13. JSON capture-pane
**What**: `capture-pane --json` returning structured output (text + cursor position, dimensions).
**Why**: Niche — raw text capture works for most agent output parsing. Subsumed by #1 if JSON mode is comprehensive.
**Effort**: Small.

## P4 — Deferred

### 14. SwarmBackend Trait Crate
**What**: Publishable Rust crate defining the tmux-subset API that psmux (and cmux shims) implement.
**Why**: Premature — only psmux implements it today. Wait for cross-platform demand.
**Effort**: Medium.

### 15. JSON-RPC Socket API
**What**: Full JSON-RPC protocol alongside text-based IPC (like cmux v2).
**Why**: `--json` on existing CLI covers 90% of the need. Full protocol rewrite is expensive.
**Effort**: Large.

### 16. Workspace-Scoped Keybindings
**What**: Per-namespace (`-L`) key binding overrides.
**Why**: Edge case — namespaces are for isolation, not UX customization.
**Effort**: Medium.
