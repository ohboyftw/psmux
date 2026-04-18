# Session Checkpoint — sync/rails-p0-p2

**Purpose:** resume the rails-sync work after session compaction. Read this first on a fresh session.

## Current State

- **Branch:** `sync/rails-p0-p2` (off `ohboy-builds`)
- **Last commit:** `da27eac fix(config): track user-set options in HashSet for correct set-option -o`
- **Tests:** `cargo test --bin psmux --lib` → 552/552 green
- **Final merge target:** `master` (after all P0/P1/P2 + tests + trace-debug done)

## Reference Docs (read these)

1. `docs/on-rails-checklist.md` — 25 rails tasks grouped P0/P1/P2/P4
2. `docs/on-rails-tests.md` — test strategy per task
3. `docs/ohboy-builds-backlog.md` — full upstream sync ledger (synced to `upstream/master@7b0fb48`)
4. `.claude/upstream-pulse/state.json` — last upstream SHA checked

## Completed This Session

| Commit | Task | What it did |
|--------|------|-------------|
| `0dec63c` | Setup | Branch + checklist doc + backlog sync + hardened switch-client detection (position-aware) |
| `59bcba6` | **P0.3** Extract `src/octal.rs` | Moved `src/remote/octal.rs` → `src/octal.rs`; added `mod octal;` to lib.rs + main.rs; updated protocol.rs call sites |
| `e4cb3f4` | **P0.5** `for_each_pane()` | Added read-only DFS helper in `src/tree.rs` (no unit test — Pane owns real PTY) |
| `da27eac` | **P0.2** set-option -o HashSet | Added `AppState.user_set_options: HashSet<String>`; `parse_set_option` inserts on write, removes on `-u`, checks on `-o`; 4 new unit tests |

## Next Up — P0.1 (heaviest P0)

**Task:** Port `allow_set_title` + OSC 0/2 pane_title propagation. Risk: Medium.

**Files to touch:**
1. `src/types.rs` — add `allow_set_title: bool` to AppState (default `false`); add `title_locked: bool` to Pane
2. `src/server/helpers.rs` — add `propagate_osc_titles(&mut AppState)` + `propagate_osc_titles_in_tree(&mut Node)` (drain OSC title from each pane's VT parser, copy to `Pane.title` if not `title_locked` and `allow_set_title` true)
3. `src/server/mod.rs` — call `propagate_osc_titles(&mut app)` in pre-auto-rename pass (~L1186, near the auto-rename loop modified in e96e9cb)
4. `src/config.rs` — parse `set -g allow-set-title` into `AppState.allow_set_title`
5. `src/server/options.rs` — wire the option into show-options output
6. `src/commands.rs` — `select-pane -T` sets `title_locked: true` + `title` = arg
7. `docs/pane-titles.md` — cherry-pick from upstream (additive, 175 lines)
8. `tests/test_issue231_osc_title_propagation.ps1` — port 7 verification tests from upstream

**Upstream reference:** combines `eb338c8` (helper extraction) + `4162d97` (default-off) + original OSC inline propagation. Useful to read:

```
git show upstream/master:src/server/helpers.rs | grep -A 30 propagate_osc_titles
git show 4162d97 --stat
```

**Tests expected:**
- Set `allow-set-title on`, write `\x1b]2;foo\x07` via send-keys, `display-message -p '#{pane_title}'` returns `foo`
- Set `allow-set-title off`, same write → `pane_title` unchanged
- `select-pane -T locked`, then OSC → still `locked` (title_locked blocks OSC)
- All existing 552 tests still pass

**Risk callouts:**
- `types.rs` is a known conflict hotspot — adding fields is safe but review construction sites
- `server/mod.rs` auto-rename loop was touched in e96e9cb — ensure propagate_osc_titles runs BEFORE auto-rename so OSC-driven titles aren't overwritten
- Pane construction sites (19 of them per P0.4 investigation) each need `title_locked: false`

## Queue After P0.1

- **P0.4** — Deferred. Bundle with control-mode port (ring buffer has no consumer today)
- **P1 batch** (task #6) — 14 upstream bug fixes. Do in 2-3 commits. Lowest risk first: items 8, 9, 10, 12, 13, 14, 15, 16, then medium-risk 6, 7, 11, 17, 18, 19
- **P2 verify** (task #7) — 30-min audit: `grep` for session.rs ns fns, PSMUX_ACTIVE guards, VTI/warmup. Write `tests/verify_parity.ps1`
- **P2 agent-adjacent** (task #8) — new-session -e (#205), PredictionViewStyle warm pane respawn, CREATE_NO_WINDOW. Each must pass `tests/validate-swarm-backend.ps1` after
- **Integration smoke tests** (task #9) — per `docs/on-rails-tests.md`
- **Trace-debug analysis** (task #10) — per `docs/on-rails-tests.md` "Trace-Debug Integration" section
- **Merge to master** (task #11) — blocked on all above

## Verify Commands

```bash
# Per-commit gate
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --bin psmux --lib

# Full sweep before merge
cargo test --tests
pwsh tests/validate-swarm-backend.ps1
pwsh tests/verify_parity.ps1   # after P2 verify item lands

# Build + install for manual test
cargo build --release
pwsh scripts/install.ps1 -NoTools -Force
```

## Task IDs (TaskList)

- `#1` P0.1 allow_set_title — **pending, pick up next**
- `#2` P0.2 set-option -o — completed
- `#3` P0.3 octal — completed
- `#4` P0.4 ring buffer — deferred
- `#5` P0.5 for_each_pane — completed
- `#6` P1 batch — pending
- `#7` P2 verify — pending
- `#8` P2 agent-adjacent — pending
- `#9` Integration smoke tests — pending
- `#10` Trace-debug analysis — pending
- `#11` Merge to master — pending (blocked by 1-10)

## Running Servers Note

At session start, 4 live psmux servers were running (user-owned sessions incl. `canopy-self`, `evolve`, `life-*`). Install script overwrote binaries successfully; running servers keep old code in memory until restart. Don't kill sessions to re-install — just let new sessions pick up new binary.

## Decisions Made (don't re-litigate)

- P0.4 deferred: ring buffer is dead code without control-mode consumer; skip the 19-pane-site churn until ready
- P0.5 no unit test: Pane owns real PTY resources, so constructing it in a test requires spawning; sibling `visit_leaves_mut` is untested for the same reason — trust by construction
- switch-client detection hardened BEYOND upstream: position-aware via existing global-flag scan, not whole-argv `any()`
- Don't commit to `master` directly — merge `sync/rails-p0-p2` when green
