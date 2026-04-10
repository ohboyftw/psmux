# upstream-pulse: sync-2026-04-10-panther

**Date:** 2026-04-10
**Previous:** sync-2026-04-09-condor

## Sources

| Source | Baseline | Current | Delta |
|--------|----------|---------|-------|
| psmux/psmux | `7f8f62f` | `c6088d5` | 7 new commits |
| Claude Code | v2.1.97 | v2.1.100 | 3 releases (v2.1.98, v2.1.100) |
| Pi Agent | v0.66.1 | v0.66.1 | no changes |

## Tier Analysis

### TIER 2: QUICK CONFIG (1 item)

- **`0b18080` — Normalize `-x=VALUE` flag form (#196)**
  - Tools like claude-squad use `has-session -t=NAME` which silently passes
  - Adds `normalize_flag_equals()` in cli.rs + app.rs + connection.rs
  - Reasoning: Important correctness fix for external tool compat. Touches 3 diverged files but the change is localized (flag parsing, not business logic)
  - Action: Manual port — add `normalize_flag_equals()` to cli.rs, wire into app.rs and connection.rs entry points. New test file `tests-rs/test_issue196_flag_equals.rs` can be cherry-picked clean
  - REGRESSION RISK: Low — flag parsing is upstream of dispatch

### TIER 3: LOW COMPLEXITY / HIGH IMPACT (1 item)

- **`6bcb9f5` — Scroll pane scrollback when copy mode disabled (PR #194)**
  - When `scroll-enter-copy-mode off`, scroll events were completely ignored. Now scrolls the pane buffer directly
  - Reasoning: Good UX improvement, moderate impact on 4 files (copy_mode.rs, input.rs, window_ops.rs, option_catalog.rs)
  - Action: Manual port — extract `scroll_pane_scrollback()` helper from copy_mode.rs, wire into input.rs and window_ops.rs scroll paths
  - Note: option_catalog.rs was deleted in ohboy-builds — add to options.rs instead

### TIER 4: FIXES (4 items)

- **`266d414` — Reset defaults_suppressed on source-file reload (#195)**
  - `unbind-key -a` flag never reset when config reloaded
  - Files: commands.rs, server/mod.rs — both diverged
  - Action: Manual port

- **`3e61a0d` — Commit session.rs namespace functions**
  - Missing function definitions broke CI
  - Files: session.rs — diverged
  - Action: Verify ohboy-builds has these functions; skip if already present

- **`86a7519` — Paste timeout to prevent terminal freeze (#197)**
  - Bracketed paste state machine had no timeout — lost close sequence = permanent freeze
  - Files: app.rs, ssh_input.rs — both diverged
  - Action: Manual port — add paste_start timestamp, 2s timeout, 1MB cap

- **`f9d443d` — unbind-key -a table support + pane improvements**
  - Multiple fixes: unbind-key -a with -T table, defaults_suppressed struct reorder, pane lifecycle
  - Files: commands.rs, config.rs, pane.rs, server/connection.rs, server/mod.rs — all diverged
  - New files: examples/enter_diag.rs, tests/test_full_feature.ps1 — can cherry-pick
  - Action: Decompose into sub-tasks; new files cherry-pick, code changes manual port

### TIER 5: FEATURES (1 item)

- **`c6088d5` — Document 13 undocumented features, 7 plugins, 4 themes**
  - Docs-only: configuration.md, features.md, plugins.md, scripting.md
  - Action: Cherry-pick or manually merge docs. Low conflict risk on docs files

## Claude Code (v2.1.98 → v2.1.100)

No TeammateTool, tmux backend, or CLAUDE_PANE_BACKEND_SOCKET changes. Notable:
- v2.1.98: `workspace.git_worktree` in status line JSON, Monitor tool, Bash security fixes
- v2.1.98: **Agent team members now inherit leader's permission mode** — relevant for swarm usage
- v2.1.100: Changelog update only

No action needed for psmux.

## Pi Agent

No changes (still v0.66.1). Pi integration work done today (R1-R7) is proactive.

## Regression Risk

No files touched by upstream overlap with our Pi integration changes (protocol.rs, backend/).
Overlap with ohboy-builds divergence is high on: server/mod.rs, pane.rs, connection.rs, types.rs.
