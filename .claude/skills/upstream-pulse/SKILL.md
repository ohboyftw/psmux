---
name: upstream-pulse
description: >
  Check for upstream changes from psmux/psmux, Claude Code, and Pi coding agent
  that need merging, fixing, or adopting into ohboy-builds. Fetches changelogs,
  git commits, and npm versions, then categorizes changes into 5 priority tiers.
  Auto-tags a git checkpoint with date+codename. Use when the user says
  "check upstream", "upstream pulse", "what's new upstream", "sync check",
  "/upstream-pulse", or at session start if >3 days since last check.
---

# upstream-pulse

Check three upstream sources for changes that need action on the `ohboy-builds` branch.

## Session-Start Nudge

At conversation start, check if `.claude/upstream-pulse/state.json` exists.
If `last_check` is older than 3 days, output:

> Last upstream-pulse check was N days ago (`sync-YYYY-MM-DD-codename`). Run `/upstream-pulse` to check for updates.

Do NOT auto-fetch. Just nudge.

## Sources

### 1. psmux/psmux (git)

```bash
git fetch upstream
git log upstream/master --oneline --since="<last_check_date>"
git diff --stat upstream/master..HEAD -- src/ crates/ Cargo.toml
```

Focus on: `src/backend/`, IPC protocol, `Cargo.toml` dependency changes.

### 2. Claude Code (web)

Fetch these URLs in order, use the first that succeeds:
1. `https://code.claude.com/docs/en/changelog`
2. `https://github.com/anthropics/claude-code/releases`
3. Fallback: `https://registry.npmjs.org/@anthropic-ai/claude-code` (JSON with version dates)

Focus on: TeammateTool changes, tmux backend protocol, agent spawning, `CLAUDE_PANE_BACKEND_SOCKET`.

### 3. Pi coding agent (web + git)

- npm registry: `https://registry.npmjs.org/@mariozechner/pi-coding-agent` — compare all versions published since `last_check` using the `time` object
- CHANGELOG: `https://raw.githubusercontent.com/badlogic/pi-mono/main/packages/ai/CHANGELOG.md`

Focus on: output format changes, CLI flags, extension API, impacts on `pi-dispatch.ps1` / `pi-swarm.ps1`.

## Priority Tiers

Categorize every change into exactly one tier:

| Tier | Name | Description |
|------|------|-------------|
| 1 | **MERGES** | Breaking changes, protocol shifts, API changes that MUST be integrated |
| 2 | **QUICK CONFIG** | Small config/option changes, easy wins, 10-line diffs |
| 3 | **LOW COMPLEXITY / HIGH IMPACT** | Straightforward changes with outsized benefit |
| 4 | **FIXES** | Bug fixes, correctness improvements |
| 5 | **FEATURES** | New capabilities to consider adopting |

For each item provide:
- **Summary** (one line)
- **Reasoning** (why this tier)
- **Action** (what specifically to do in ohboy-builds)

## Analysis Prompt

Read `prompt-template.md` in this skill directory for the full LLM analysis prompt.
Feed it the raw fetched data (commits, changelogs, diffs) along with the tier definitions.

## Workflow

1. **Read state** — Load `.claude/upstream-pulse/state.json` for baselines (or initialize if first run)
2. **Fetch sources** — All three sources are independent; fetch them in parallel when possible
3. **Analyze** — Use the prompt template to categorize each change into tiers
4. **Output report** — Print the tiered report to terminal (see format below)
5. **Save report** — Write to `.claude/upstream-pulse/reports/sync-<date>-<codename>.md`
6. **Tag** — Create an annotated git tag `sync-<date>-<codename>`
7. **Update state** — Write new baselines to `state.json`

### First Run (no state.json)

- Git: use `--since=30.days.ago`
- npm/web: fetch latest version as baseline only
- Create `state.json` with current values
- Label report as "initial baseline"

### Failure Handling

- If a source fails (network error, 404, format change), report the failure inline
- Continue with remaining sources
- Only update `state.json` baselines for sources that succeeded
- Still create the git tag (note the failure in the tag message)

## Output Format

```
━━━ upstream-pulse ━━━ sync-2026-03-24-falcon ━━━

Sources checked:
  psmux/psmux    10edfe1 → a3bc4f2  (7 new commits)
  Claude Code    v2.1.79 → v2.2.0   (1 release)
  Pi Agent       v0.8.2  → v0.8.2   (no changes)

┌─ TIER 1: MERGES (N items) ─────────────────────
│ ● source: summary
│   → action to take
│
├─ TIER 2: QUICK CONFIG (N items) ───────────────
│ ...
│
├─ TIER 3: LOW COMPLEXITY / HIGH IMPACT (N items)
│ ...
│
├─ TIER 4: FIXES (N items) ─────────────────────
│ ...
│
└─ TIER 5: FEATURES (N items) ──────────────────
  ...

━━━ Tagged: sync-2026-03-24-codename ━━━
```

## State File

**Path**: `.claude/upstream-pulse/state.json`

```json
{
  "last_check": "2026-03-24T10:30:00Z",
  "tag": "sync-2026-03-24-falcon",
  "codename_index": 7,
  "upstream_sha": "10edfe1",
  "claude_code_version": "2.1.79",
  "pi_version": "0.8.2",
  "sources": {
    "claude_code_changelog": "https://code.claude.com/docs/en/changelog",
    "claude_code_releases": "https://github.com/anthropics/claude-code/releases",
    "claude_code_npm": "https://registry.npmjs.org/@anthropic-ai/claude-code",
    "pi_npm": "https://registry.npmjs.org/@mariozechner/pi-coding-agent",
    "pi_changelog": "https://raw.githubusercontent.com/badlogic/pi-mono/main/packages/ai/CHANGELOG.md"
  }
}
```

On partial success, only update baselines for sources that succeeded.

## Tagging

1. Read `codenames.txt` — pick name at `codename_index`, increment and wrap
2. Tag format: `sync-YYYY-MM-DD-codename`
3. If same date already tagged: append `.2`, `.3`, etc.
4. Tag message includes: source summary, item count per tier, Tier 1-2 items in full (Tier 3-5 counts only)
5. Update `state.json` with new tag name and codename index

## Reports

Saved to `.claude/upstream-pulse/reports/sync-<date>-<codename>.md`

Keep last 20 reports. Delete older ones automatically.
