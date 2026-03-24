# upstream-pulse — Design Spec

**Date**: 2026-03-24
**Status**: Approved
**Branch**: ohboy-builds

## Problem

`ohboy-builds` is 115+ commits ahead of upstream `psmux/psmux`, with no automation to detect when upstream psmux, Claude Code, or Pi coding agent make changes that require action. Changes in any of these three sources can introduce breaking protocol shifts, useful fixes, or features worth porting — and currently all discovery is manual.

## Solution

A Claude Code skill (`/upstream-pulse`) that fetches changes from three sources, uses LLM analysis to categorize each change into 5 priority tiers, outputs a tiered action report, and auto-tags a git checkpoint with a date+codename identifier.

## Sources

### 1. psmux/psmux (git)
- `git fetch upstream` then `git log upstream/master --since=<last-check-date>`
- Captures commit messages, changed files, diff stats
- Special attention to: `src/backend/`, IPC protocol files, `Cargo.toml` dependency changes

### 2. Claude Code (web)
- Primary: Fetch `https://code.claude.com/docs/en/changelog` — parse for new entries since last check
- Secondary: Fetch `https://github.com/anthropics/claude-code/releases` — get release notes
- Fallback: Query npm registry `https://registry.npmjs.org/@anthropic-ai/claude-code` for version metadata and dates
- Focus on: TeammateTool changes, tmux backend protocol, agent spawning, `CLAUDE_PANE_BACKEND_SOCKET` references
- URLs are configurable in `state.json` under a `sources` key so they can be updated without editing the skill

### 3. Pi coding agent (web + git)
- Fetch npm registry `https://registry.npmjs.org/@mariozechner/pi-coding-agent` — compare all versions published since `last_check` (using the `time` object), not just latest vs stored
- Fetch Pi CHANGELOG from `https://raw.githubusercontent.com/badlogic/pi-mono/main/packages/ai/CHANGELOG.md` — parse new entries since last check (branch-based URL, not commit-pinned)
- Focus on: output format changes, CLI flags, extension API, anything affecting `pi-dispatch.ps1` / `pi-swarm.ps1`

## Priority Tiers

All changes are categorized into 5 tiers, highest priority first:

| Tier | Name | Description | Examples |
|------|------|-------------|----------|
| 1 | **Merges** | Breaking changes, protocol shifts, API changes that must be integrated | IPC protocol change, TeammateTool contract change |
| 2 | **Quick Config** | Small configuration/option changes, easy wins | New `set -g` option, CLI flag addition |
| 3 | **Low Complexity / High Impact** | Straightforward changes with outsized benefit | Performance fix, stability improvement |
| 4 | **Fixes** | Bug fixes, correctness improvements | Race condition fix, edge case handling |
| 5 | **Features** | New capabilities to consider adopting | New command, UI enhancement |

## LLM Analysis

After fetching raw data (commits, changelogs, diffs), all content is fed to Claude with the tier definitions and project context. For each change item, Claude assigns:

- **Tier** (1-5)
- **Summary** (one line)
- **Reasoning** (why this tier)
- **Action** (what specifically needs doing in ohboy-builds)

## Output Format

Printed to terminal as a structured report:

```
━━━ upstream-pulse ━━━ sync-2026-03-24-falcon ━━━

Sources checked:
  psmux/psmux    10edfe1 → a3bc4f2  (7 new commits)
  Claude Code    v2.1.79 → v2.2.0   (1 release)
  Pi Agent       v0.8.2  → v0.8.2   (no changes)

┌─ TIER 1: MERGES (2 items) ─────────────────────
│ ● upstream: IPC protocol changed from msgpack to JSON
│   → Must update src/backend/protocol.rs to match
│ ● claude-code: TeammateTool now sends `ready` heartbeat
│   → Add heartbeat handler to CustomPaneBackend
│
├─ TIER 2: QUICK CONFIG (1 item) ────────────────
│ ● upstream: new `set -g pane-border-style` option
│   → Port config parser change, 10-line diff
│
├─ TIER 3: LOW COMPLEXITY / HIGH IMPACT (0 items)
│   (none)
│
├─ TIER 4: FIXES (3 items) ──────────────────────
│ ● upstream: fix pane removal focus loss (#140)
│   → Already diverged, verify our impl handles this
│ ● upstream: fix resize race on ConPTY
│   → Cherry-pick candidate, affects stability
│ ● claude-code: fix agent spawn timeout on slow machines
│   → No action needed, client-side fix
│
└─ TIER 5: FEATURES (1 item) ────────────────────
  ● upstream: new `display-popup` command
    → Consider porting, useful for agent status overlays

━━━ Tagged: sync-2026-03-24-falcon ━━━
```

Report also saved to `.claude/upstream-pulse/reports/sync-<date>-<codename>.md`.

## State Management

### Baseline file: `.claude/upstream-pulse/state.json`

```json
{
  "last_check": "2026-03-24T10:30:00Z",
  "tag": "sync-2026-03-24-falcon",
  "codename_index": 7,
  "upstream_sha": "10edfe1",
  "claude_code_version": "2.1.79",
  "pi_version": "0.8.2"
}
```

Updated at the end of each successful run. On partial success (some sources fetched, others failed), `state.json` is updated only for the sources that succeeded — failed sources retain their previous baseline so they are re-checked next run.

### First Run

When `state.json` does not exist:
- Git sources: use `--since=30.days.ago` for the initial scan
- npm/web sources: fetch only the latest version as baseline
- Create `state.json` with current values
- Label the report as "initial baseline" and tag accordingly
- The first run establishes the checkpoint — subsequent runs diff against it

### Git tagging

1. Generate codename — sequential pick from `codenames.txt` (wraps to index 0 after exhausting list; codename reuse is safe because the date prefix ensures tag uniqueness)
2. Create tag: `sync-2026-03-24-falcon`
3. If same date already has a tag, append suffix: `sync-2026-03-24-falcon.2`
4. Tag message includes: source summary line (versions/SHAs), item count per tier, and Tier 1-2 items in full (Tier 3-5 as counts only)
5. Update `state.json` with new baseline

## Session-Start Nudge

When a conversation starts and `state.json` exists with `last_check` older than 3 days:

> "Last upstream-pulse check was 5 days ago (`sync-2026-03-19-lynx`). Run `/upstream-pulse` to check for updates."

Passive reminder only — no auto-fetch.

**Mechanism**: Add a line to the project's `CLAUDE.md` instructing Claude to check `.claude/upstream-pulse/state.json` age at session start and surface the nudge if >3 days stale.

## Skill File Structure

```
.claude/skills/upstream-pulse/
├── SKILL.md              — Skill definition (trigger, workflow, tier rubric)
├── codenames.txt          — 50 animal codenames, one per line
└── prompt-template.md     — LLM analysis prompt with tier rubric and project context
```

State & reports (not checked into git — add `.claude/upstream-pulse/` to `.gitignore`):

```
.claude/upstream-pulse/
├── state.json             — Last-check baseline
└── reports/
    └── sync-2026-03-24-falcon.md
```

## Invocation

- **Primary**: `/upstream-pulse` slash command
- **Session-start**: Nudge if >3 days since last check
- **Trigger phrases**: "check upstream", "upstream pulse", "what's new upstream", "sync check"

## Design Decisions

1. **Prompt-driven, no scripts** — The entire flow uses Claude's existing tools (git commands, WebFetch). No PowerShell scripts needed since data fetching, analysis, and tagging are all tool calls.
2. **Sequential codenames** — Predictable, memorable, no collisions. Cycling through 50 names means ~1 year of weekly checks before repeating.
3. **LLM categorization over heuristics** — Sources are heterogeneous (git commits, markdown changelogs, npm metadata). Keyword heuristics would be brittle; LLM analysis handles the judgment calls naturally.
4. **Independent sources** — All three sources are independent and can be fetched in any order, including concurrently when the tool infrastructure supports parallel calls.
5. **Reports saved as markdown** — Human-readable, git-friendly, searchable. Can be referenced in future sessions. Retention: keep last 20 reports; older ones are deleted automatically.
6. **Graceful degradation** — If a source fails (network error, 404, changed format), the skill reports the failure inline, continues with remaining sources, and only updates `state.json` baselines for sources that succeeded. The git tag is still created (marking what was checked), with the failure noted in the tag message.
7. **Source URLs configurable** — Claude Code and Pi URLs stored in `state.json` under a `sources` key, allowing updates without editing the skill files.
