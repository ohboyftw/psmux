# PRD: Ralph Loop — Iterative Agent Loop Plugin for Claude Code

**Version:** 1.0.0  
**Author:** Aravind / OhboyConsultancy FZ LLC  
**Date:** 2026-02-14  
**Status:** Reference Implementation

---

## 1. Executive Summary

Ralph Loop is a Claude Code plugin that implements the "Ralph Wiggum" iterative development methodology — a continuous agent loop where Claude Code works on a task, attempts to exit, gets intercepted by a stop hook, and is fed the same prompt again to iterate on its own prior work. This creates a self-referential feedback loop enabling progressive refinement without external bash wrappers.

This document specifies the complete architecture, data model, hook lifecycle, and completion semantics for a clean-room reference implementation that is compatible with Claude Code's current security model (post CVE-2025-54795).

---

## 2. Problem Statement

Claude Code sessions are inherently single-pass: the agent processes a prompt, performs work, and exits. For complex tasks requiring iterative refinement — TDD cycles, large refactors, multi-phase builds — users must manually re-invoke the agent or wrap it in external bash loops, losing session context each time.

**Core pain points:**

- Manual re-invocation is tedious and loses conversational momentum
- External `while true` bash loops lose session-internal context
- No structured way to define completion criteria for autonomous work
- No iteration tracking or safety limits for runaway loops

---

## 3. Solution Overview

Ralph Loop intercepts Claude Code's session exit via the **Stop Hook** mechanism and re-injects the original prompt, creating an in-session iteration loop. The agent observes its prior work through the filesystem and git history, enabling self-correction without explicit memory of previous iterations.

### 3.1 Architecture Principle

```
User invokes /ralph-loop "task" --max-iterations 20
    │
    ▼
setup-ralph-loop.sh parses args, creates state file
    │
    ▼
┌─────────────────────────────────────┐
│  Claude works on task (iteration N) │◄──────────┐
│  Reads files, git log, tests, etc.  │           │
└──────────────┬──────────────────────┘           │
               │ Agent tries to exit              │
               ▼                                  │
┌─────────────────────────────────────┐           │
│  stop-hook.sh intercepts exit       │           │
│  • Reads state file                 │           │
│  • Checks exit conditions           │           │
│  • Increments iteration counter     │           │
│  • Re-injects original prompt       │───────────┘
└──────────────┬──────────────────────┘
               │ Exit conditions met
               ▼
         Session ends normally
```

### 3.2 Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| State in markdown file (YAML frontmatter) | Human-readable, inspectable, compatible with Claude's Read tool |
| Stop hook (not claim hook) | Intercepts exit, not input — loop happens after work is done |
| No subagents | Loop runs in the main session to preserve full context |
| Implicit hook activation via state file presence | No explicit hook registration needed; presence of file = loop active |
| Single-line bash in command files | Post CVE-2025-54795, multi-line bash in command `.md` files is blocked |

---

## 4. Functional Requirements

### 4.1 Commands

#### FR-1: `/ralph-loop` — Start a loop

| Attribute | Value |
|-----------|-------|
| Syntax | `/ralph-loop PROMPT [--max-iterations N] [--completion-promise TEXT]` |
| PROMPT | Multi-word task description (no quotes required) |
| `--max-iterations` | Integer ≥ 0. Default: 0 (unlimited). Safety ceiling for iterations |
| `--completion-promise` | Quoted string. When agent outputs `<promise>TEXT</promise>`, loop exits |
| `-h, --help` | Display usage |
| Allowed tools | `Bash(${CLAUDE_PLUGIN_ROOT}/scripts/setup-ralph-loop.sh:*)` |
| Visibility | Hidden from slash command tool (`hide-from-slash-command-tool: true`) |

**Acceptance criteria:**
- Creates `.claude/ralph-loop.local.md` state file
- Displays iteration tracking info and safety warnings
- Displays completion promise instructions if set
- Warns if neither `--max-iterations` nor `--completion-promise` is set

#### FR-2: `/cancel-ralph` — Stop an active loop

| Attribute | Value |
|-----------|-------|
| Syntax | `/cancel-ralph` |
| Behavior | Removes `.claude/ralph-loop.local.md`, allowing normal exit |
| Allowed tools | `Bash(test -f ...)`, `Bash(rm ...)`, `Read(...)` on state file |
| Visibility | Hidden from slash command tool |

**Acceptance criteria:**
- Removes state file if it exists
- Confirms cancellation to user
- No-op with message if no active loop

### 4.2 Stop Hook

#### FR-3: Stop hook intercepts session exit

**Trigger:** Claude Code fires the `Stop` hook when the agent attempts to end the session.

**Behavior:**
1. Check if `.claude/ralph-loop.local.md` exists and `active: true`
2. Parse YAML frontmatter for `iteration`, `max_iterations`, `completion_promise`
3. **Check max iterations:** If `max_iterations > 0` and `iteration >= max_iterations`, allow exit
4. **Check completion promise:** If `completion_promise` is set, scan the agent's last output (passed via `$CLAUDE_TRANSCRIPT` or stop hook stdin) for `<promise>PROMISE_TEXT</promise>`. If found, allow exit
5. **Otherwise:** Increment `iteration` in the state file, output the original prompt text to stdout (which becomes the next user message), and signal "block exit"
6. The hook communicates via **exit code**: `0` = allow exit, non-zero = block exit and use stdout as new prompt

#### FR-4: Stop hook JSON protocol

Claude Code's stop hook protocol expects a JSON response on stdout:

```json
{
  "decision": "block",
  "reason": "Ralph loop iteration 3/20 — continuing",
  "message": "<original prompt text>"
}
```

Or to allow exit:

```json
{
  "decision": "approve",
  "reason": "Ralph loop complete — max iterations reached"
}
```

### 4.3 State File

#### FR-5: State file format

**Path:** `.claude/ralph-loop.local.md`  
**Naming convention:** `.local.md` suffix ensures it's gitignored by Claude Code conventions.

```yaml
---
active: true
iteration: 1
max_iterations: 20
completion_promise: "COMPLETE"
started_at: "2026-02-14T10:30:00Z"
---

Build a REST API for todos. When complete:
- All CRUD endpoints working
- Tests passing (coverage > 80%)
- Output: <promise>COMPLETE</promise>
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `active` | boolean | `true` | Loop is active (always true when file exists) |
| `iteration` | integer | `1` | Current iteration (1-indexed, incremented by stop hook) |
| `max_iterations` | integer | `0` | 0 = unlimited |
| `completion_promise` | string \| null | `null` | Exact text to match inside `<promise>` tags |
| `started_at` | ISO 8601 | Current UTC | When loop was initiated |

---

## 5. Non-Functional Requirements

### 5.1 Security

| Requirement | Implementation |
|-------------|----------------|
| No multi-line bash in command files | Single-line script invocation only (CVE-2025-54795 compliance) |
| No `$()` command substitution in commands | All logic lives in `.sh` scripts, not inline |
| Restricted tool access | Each command explicitly lists allowed tools |
| Hidden commands | Prevents agent from self-invoking loop commands |

### 5.2 Observability

- State file is human-readable markdown
- Setup script outputs clear status with iteration limits and promise text
- Stop hook logs iteration progress
- `cat .claude/ralph-loop.local.md` shows current state at any time

### 5.3 Safety

- `--max-iterations` is the **primary** safety mechanism (exact string matching for promises can be fragile)
- Warning displayed when neither safety mechanism is configured
- `/cancel-ralph` provides emergency exit
- State file uses `.local.md` extension to avoid accidental git commits

---

## 6. Hooks Configuration

The plugin registers hooks via `hooks/hooks.json`:

```json
{
  "hooks": [
    {
      "matcher": "Stop",
      "hooks": [
        {
          "type": "command",
          "command": "${CLAUDE_PLUGIN_ROOT}/hooks/stop-hook.sh"
        }
      ]
    }
  ]
}
```

**Hook contract:**
- **Input:** Stop hook receives the transcript/last assistant message via stdin (JSON)
- **Output:** JSON on stdout with `decision` ("approve" or "block") and optional `message`
- **State mutation:** Hook reads and writes `.claude/ralph-loop.local.md`

---

## 7. User Journeys

### 7.1 Basic TDD Loop

```bash
/ralph-loop Implement user authentication with JWT. Follow TDD:
  write failing tests, implement, run tests, fix failures.
  Output <promise>ALL TESTS PASSING</promise> when done.
  --completion-promise "ALL TESTS PASSING" --max-iterations 15
```

**Expected behavior:** Claude writes tests, implements auth, runs tests, finds failures, fixes them — repeating up to 15 times until all tests pass and it outputs the promise.

### 7.2 Phased Build

```bash
/ralph-loop Build an e-commerce API in phases:
  Phase 1: User auth (JWT, tests)
  Phase 2: Product catalog (CRUD, search, tests)
  Phase 3: Shopping cart (add/remove, tests)
  Output <promise>ALL PHASES COMPLETE</promise> when done.
  --completion-promise "ALL PHASES COMPLETE" --max-iterations 30
```

### 7.3 Emergency Cancel

```bash
# Loop seems stuck or going in wrong direction
/cancel-ralph
```

---

## 8. Edge Cases and Failure Modes

| Scenario | Behavior |
|----------|----------|
| Agent outputs promise text outside `<promise>` tags | Loop continues (exact XML tag matching required) |
| Agent lies about completion to escape loop | Prompt instructs agent not to; but `--max-iterations` provides hard ceiling |
| State file manually deleted mid-loop | Next exit attempt proceeds normally (no state file = no interception) |
| Multiple concurrent Claude sessions in same directory | State file is shared — only one loop at a time per project |
| `--max-iterations 0` with no promise | Infinite loop; only `/cancel-ralph` or manual state file deletion stops it |
| Agent produces no output matching promise | Loop continues until max iterations |
| Permission errors on state file | Setup script validates `.claude/` directory exists and is writable |

---

## 9. Future Considerations

- **Multi-promise support:** Allow multiple distinct completion signals (e.g., "SUCCESS" vs "BLOCKED") with different exit behaviors
- **Progress tracking:** Parse agent output for structured progress markers and maintain a progress log
- **Nested loops:** Support sub-loops for individual phases within a larger task
- **Metrics collection:** Track token usage, time per iteration, convergence patterns
- **Hook-based stop hook:** When Claude Code's plugin system supports native stop hook registration (rather than implicit file-based activation), migrate to explicit registration

---

## 10. Success Metrics

| Metric | Target |
|--------|--------|
| Loop activates correctly on first invocation | 100% |
| Stop hook correctly blocks exit and re-injects prompt | 100% |
| Max iterations respected exactly | ±0 iterations |
| Completion promise detection accuracy | Exact match, no false positives |
| Clean cancellation via `/cancel-ralph` | Immediate, no residual state |
| No security warnings from Claude Code sandbox | Zero CVE-2025-54795 triggers |
