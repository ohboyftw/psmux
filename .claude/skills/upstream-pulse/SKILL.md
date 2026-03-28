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

## Auto-Merge Safe Mode

When the user runs `/upstream-pulse --auto-merge-safe`, perform the normal analysis first, then automatically cherry-pick zero-conflict commits.

### Identifying Safe Commits

For each new commit from `upstream/master`, check if it ONLY adds new files:

```bash
# List files that are Added only (safe)
git diff-tree --no-commit-id --diff-filter=A -r <sha>

# List files that are Modified, Deleted, Renamed, Copied, or Type-changed (unsafe)
git diff-tree --no-commit-id --diff-filter=MDRCT -r <sha>
```

A commit is **safe to auto-merge** when:
- It has zero MDRCT entries (only adds new files)
- OR it modifies only files that have NOT diverged between `upstream/master` and `HEAD`

### Cherry-Pick Procedure

For each safe commit, in chronological order:

1. `git cherry-pick <sha> --no-edit`
2. If conflict: `git cherry-pick --abort`, mark as skipped
3. Run `cargo check` — if it fails: `git revert HEAD --no-edit`, mark as failed
4. If success: mark as auto-merged

### Report Annotations

Add an `AUTO-MERGE RESULTS` box after the tier report:

```
┌─ AUTO-MERGE RESULTS ───────────────────────────
│ ✓ Merged:  403a936 test: StrictMode compat tests
│ ✓ Merged:  17d4422 squelch visibility tests
│ ✗ Skipped: 43dd68f popup-as-pane (modifies existing files)
│ ✗ Failed:  e000662 window index prompt (cargo check failed)
│
│ Auto-merged: 2/4 eligible, 2 skipped
└─────────────────────────────────────────────────
```

Annotate tier items with `[auto-merged]` or `[needs manual merge]`.

## Auto Task Creation

After generating the tiered report, automatically create tasks using the `TaskCreate` tool.

### Task Mapping

| Tier | Strategy | Subject format |
|------|----------|----------------|
| 1 | One task per item | `MERGE: <summary>` |
| 2 | One task per item | `CONFIG: <summary>` |
| 3 | One task per item | `<summary>` |
| 4 | One combined task | `Merge Tier 4 fixes from upstream (<N> items)` |
| 5 | One combined task | `Port Tier 5 features from upstream (<N> items)` |

### Task Dependencies

- Tier 1 tasks block all Tier 2-3 tasks (use `addBlockedBy`)
- All Tier 1-3 merge tasks block Tier 4-5 combined tasks
- Within the same tier, tasks are independent (no ordering)

### Auto-Merged Items

If `--auto-merge-safe` was used:
- Items that were auto-merged: create the task AND immediately mark it as `completed`
- Items that failed auto-merge: create as `pending` with failure reason in description

### Task Description Format

Each task description includes:
- Upstream commit SHA(s)
- Files changed
- Risk level (from tier analysis)
- Specific action to take (merge, cherry-pick, port, verify)
- For combined Tier 4-5 tasks: bullet list of all items

## Desktop Notifications

When Tier 1 or Tier 2 items are found, emit OSC 99 escape sequences to trigger Windows toast notifications via psmux's built-in notification handler.

### Notification Rules

| Condition | Notification |
|-----------|-------------|
| Tier 1 items found | Always notify |
| Tier 2 items found | Notify |
| Tier 3-5 only | No notification (silent) |

### Escape Sequence

Emit **after** the full report is printed, using `printf` (not `echo`):

```bash
# Tier 1 notification
printf '\033]99;psmux upstream: %d Tier 1 merge(s) needed — %s\007' "$count" "$first_item_summary"

# Tier 2 notification
printf '\033]99;psmux upstream: %d quick config item(s) — %s\007' "$count" "$first_item_summary"
```

When running in Claude Code (not a raw terminal), use the Bash tool to emit:

```bash
printf '\033]99;psmux upstream: 1 Tier 1 merge needed — popup-as-pane refactor\007'
```

The psmux server's VT100 parser will catch the OSC 99 sequence and fire a Windows toast notification via PowerShell.

## Scheduled Runs

Use Claude Code's `/schedule` command to run upstream-pulse automatically.

### Setup

```
/schedule create --name upstream-pulse-daily --cron "0 9 * * *" --prompt "/upstream-pulse --auto-merge-safe"
```

This runs every day at 9 AM and:
1. Fetches all three upstream sources
2. Categorizes changes into tiers
3. Auto-merges zero-conflict commits (new test files, docs)
4. Creates tasks for remaining items
5. Fires desktop notifications for Tier 1-2 items
6. Saves report and tags the checkpoint

### Management

```bash
# List scheduled runs
/schedule list

# Pause the schedule
/schedule update --name upstream-pulse-daily --enabled false

# Resume the schedule
/schedule update --name upstream-pulse-daily --enabled true

# Change schedule (e.g. twice daily)
/schedule update --name upstream-pulse-daily --cron "0 9,17 * * *"

# Delete the schedule
/schedule delete --name upstream-pulse-daily
```

### What Each Scheduled Run Produces

- **Report file**: `.claude/upstream-pulse/reports/sync-<date>-<codename>.md`
- **Git tag**: `sync-<date>-<codename>` with tier summary in message
- **Cherry-picks**: Zero-conflict commits merged automatically
- **Tasks**: Created/updated for remaining manual work
- **Toast notification**: If Tier 1-2 items detected
- **Updated state**: `state.json` baselines advanced

## Feature Regression Check

After any cherry-pick or merge, cross-reference upstream changes against the ohboy-builds feature registry at `docs/ohboy-builds-features.md`.

### Workflow

1. **Load the registry**: Read `docs/ohboy-builds-features.md`
2. **Match risk files**: For each merged/cherry-picked commit, check which files changed. Compare against the "Risk from upstream" column in the registry.
3. **Flag at-risk features**: If a commit touches risk files for a feature, add a `REGRESSION RISK` annotation to the report and the corresponding task.
4. **Run automated checks**: After all merges complete, run:
   - `cargo test` (catches compilation and unit regressions)
   - `cargo clippy -- -D warnings`
   - `pwsh tests/validate-swarm-backend.ps1` (if backend files touched)
5. **Manual verification tasks**: For each at-risk feature that can't be fully tested automatically, create a task with the manual verification steps from the registry.

### Report Annotations

Add a `REGRESSION RISK` box after the tier report (before AUTO-MERGE RESULTS if present):

```
┌─ REGRESSION RISK ──────────────────────────────
│ ⚠ Three-State Focus Borders — rendering.rs touched by ee35684
│   Verify: alt-tab dimming still works
│ ⚠ CustomPaneBackend — types.rs touched by b68962b
│   Verify: pwsh tests/validate-swarm-backend.ps1
│ ✓ DCS Passthrough — no risk files touched
│ ✓ Session Resurrection — no risk files touched
└─────────────────────────────────────────────────
```

### Task Annotations

When creating merge tasks, append regression risk to the description:

```
REGRESSION RISK: This commit touches src/types.rs which is a risk file for:
- CustomPaneBackend (dispatcher uses CtrlReq enum)
- Remote Control Mode (protocol types)
- Agent Orchestration (wait-pane commands)
After merge, run: pwsh tests/validate-swarm-backend.ps1
```

### Registry Maintenance

When a new feature lands on ohboy-builds, add it to `docs/ohboy-builds-features.md` with:
- Files it owns
- Risk files from upstream
- Test command (automated)
- Manual verification steps
