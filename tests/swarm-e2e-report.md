# psmux Swarm E2E Test Report

**Generated:** 2026-03-19 13:47:31
**Platform:** Microsoft Windows NT 10.0.26200.0
**PowerShell:** 7.5.4
**psmux binary:** D:\Home\psmux\tests\..\target\release\psmux.exe
**psmux version:** psmux 3.2.0
**Duration:** 254s
**Phases run:** 1, 2, 3, 4, 5, 6

## Summary

| Metric | Count |
|--------|-------|
| Total | 64 |
| Passed | 63 |
| Failed | 1 |
| Skipped | 0 |
| Pass Rate | 98.4% |

## Phase Results

### Phase 1: Prerequisites [PASS]

| Test | Name | Status | Detail |
|------|------|--------|--------|
| 1.1 | psmux binary exists | PASS | - |
| 1.2 | psmux is on PATH (as psmux or tmux) | PASS | - |
| 1.3 | tmux alias resolves to psmux binary | PASS | - |
| 1.4 | psmux version is 3.2+ (Claude Code requirement) | PASS | - |
| 1.5 | psmux -V exit code is 0 | PASS | - |
| 1.6 | node.js is available (for .js agent files) | PASS | - |
| 1.7 | claude CLI is available | PASS | - |
| 1.8 | git is available (for worktree tests) | PASS | - |
| 1.9 | Current directory is a git repo | PASS | - |

### Phase 2: Environment Detection [FAIL]

| Test | Name | Status | Detail |
|------|------|--------|--------|
| 2.1 | $TMUX is set inside psmux session | PASS | - |
| 2.2 | $TMUX format contains /tmp/tmux- pattern | PASS | - |
| 2.3 | $TMUX_PANE is set with %N format | PASS | - |
| 2.4 | $TMUX propagates to child panes | FAIL | Returned false |
| 2.5 | has-session returns 0 for existing session | PASS | - |
| 2.6 | has-session returns non-zero for missing session | PASS | - |
| 2.7 | $CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS is set | PASS | - |

### Phase 3: Core Backend Commands [PASS]

| Test | Name | Status | Detail |
|------|------|--------|--------|
| 3.1 | new-session -s name -d (detached creation) | PASS | - |
| 3.2 | kill-session removes session cleanly | PASS | - |
| 3.3 | list-sessions shows created session | PASS | - |
| 3.4 | split-window -h returns %N pane ID | PASS | - |
| 3.5 | split-window -v returns %N pane ID | PASS | - |
| 3.6 | list-panes includes %N identifiers | PASS | - |
| 3.7 | list-panes marks active pane with (active) | PASS | - |
| 3.8 | kill-pane removes only targeted pane | PASS | - |
| 3.9 | send-keys delivers text to targeted pane | PASS | - |
| 3.10 | send-keys -l literal mode (Enter = text, not keypress) | PASS | - |
| 3.11 | send-keys pane isolation (A does not leak to B) | PASS | - |
| 3.12 | select-layout tiled accepted | PASS | - |
| 3.13 | select-layout main-vertical accepted | PASS | - |
| 3.14 | select-pane -t %N targets specific pane | PASS | - |
| 3.15 | display-message -p #{pane_id} returns %N | PASS | - |
| 3.16 | capture-pane -p returns pane content | PASS | - |
| 3.17 | #{pane_pid} returns numeric PID | PASS | - |
| 3.18 | #{session_name} returns session name | PASS | - |

### Phase 4: Swarm Lifecycle [PASS]

| Test | Name | Status | Detail |
|------|------|--------|--------|
| 4.1 | Create swarm session with 3 agent panes | PASS | - |
| 4.2 | Apply tiled layout to swarm session | PASS | - |
| 4.3 | Inject prompts into all 3 agent panes | PASS | - |
| 4.4 | Agent outputs result marker (simulating inbox write) | PASS | - |
| 4.5 | list-panes shows all agent panes alive | PASS | - |
| 4.6 | Kill one agent pane, others survive | PASS | - |
| 4.7 | capture-pane on surviving agent returns content | PASS | - |
| 4.8 | Create git worktrees for agent isolation | PASS | - |
| 4.9 | Agent can cd to worktree and work | PASS | - |
| 4.10 | Kill swarm session cleanly | PASS | - |

### Phase 5: Edge Cases & Stress [PASS]

| Test | Name | Status | Detail |
|------|------|--------|--------|
| 5.1 | send-keys handles 500+ char prompt | PASS | - |
| 5.2 | send-keys handles 2000+ char prompt (Claude Code agent prompts) | PASS | - |
| 5.3 | send-keys handles double quotes | PASS | - |
| 5.4 | send-keys handles single quotes | PASS | - |
| 5.5 | send-keys handles special shell chars (|, &, ;, >) | PASS | - |
| 5.6 | send-keys handles backslash-colon (POSIX escape pattern) | PASS | - |
| 5.7 | 5 rapid splits produce 5 unique pane IDs (no race condition) | PASS | - |
| 5.8 | Rapid send-keys to multiple panes (no cross-contamination) | PASS | - |
| 5.9 | Detached session persists and retains pane content | PASS | - |
| 5.10 | Can create 6+ panes (practical agent count) | PASS | - |

### Phase 6: Failure Modes [PASS]

| Test | Name | Status | Detail |
|------|------|--------|--------|
| 6.1 | psmux sets TMUX (prevents in-process fallback) | PASS | - |
| 6.2 | tmux command works inside psmux pane | PASS | - |
| 6.3 | send-keys handles && chaining (cd && env ... cmd) | PASS | - |
| 6.4 | send-keys handles env KEY=VALUE syntax | PASS | - |
| 6.5 | Session survives rapid create-kill-create cycle | PASS | - |
| 6.6 | Stale port file doesn't block new session | PASS | - |
| 6.7 | Pane shell is PowerShell (not cmd.exe) | PASS | - |
| 6.8 | Pane inherits parent PATH | PASS | - |
| 6.9 | Resize after layout change doesn't crash | PASS | - |
| 6.10 | resize-pane with percentage (%) accepted | PASS | - |

## Failed Tests Analysis

### [2.4] $TMUX propagates to child panes

**Error:** `Returned false`

**Category:** Environment variable not set
**Impact:** Claude Code will fall back to in-process agent spawning (invisible agents)
**Fix:** Check psmux session init sets `TMUX`, `TMUX_PANE` in child shell env
**Source:** `src/server/mod.rs` or `src/pane.rs` (env propagation to child processes)

## Swarm Readiness Assessment

**READY** — All critical backend commands and swarm lifecycle tests pass.
psmux can serve as the tmux spawn backend for Claude Code agent teams.

**Next steps:**
1. Run `pwsh scripts/Start-ClaudeTeams.ps1` to launch Claude Code with agent teams
2. Inside Claude Code, ask it to spawn teammates — they should appear as visible psmux panes
3. Try `/spawn-swarm` for full multi-agent orchestration

## Environment Details

| Variable | Value |
|----------|-------|
| `tmux` on PATH | C:\Users\aravi\.cargo\bin\tmux.exe |
| `psmux` on PATH | C:\Users\aravi\.cargo\bin\psmux.exe |
| `node` on PATH | C:\Program Files\nodejs\node.exe |
| `claude` on PATH | C:\Users\aravi\.local\bin\claude.exe |
| `git` on PATH | C:\Program Files\Git\mingw64\bin\git.exe |
| Windows Terminal | Yes |
| `TERM_PROGRAM` | not set |

---
*Report generated by `tests/test_swarm_e2e.ps1`*
