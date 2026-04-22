# FleetView — Design Doc (v1: library + MCP + CLI)

**Status:** proposed
**Owner:** ohboyftw
**Target:** psmux `ohboy-builds` branch (new crates `psmux-fleet`, `psmux-fleet-mcp`; new subcommand `psmux fleet`)
**Date:** 2026-04-22
**Supersedes:** `2026-04-21-perch-mcp-design.md` (deleted on merge of this spec)

---

## 1. What FleetView is

FleetView is the **aggregator over psmux's fleet primitives**. It produces one structured model of every workspace in a session — running, awaiting, terminal — and exposes controlled operations to spawn, inspect, merge, and kill them. That model and those operations are rendered through interchangeable frontends.

v1 ships three frontends from a shared core library:

- **MCP server** (`psmux-fleet-mcp`) — stdio JSON-RPC for LLM sessions (Claude Code, future clients).
- **CLI subcommand tree** (`psmux fleet <verb>`) — for scripts, humans, and ambient terminal use.
- **Library** (`psmux-fleet`) — typed public API for third-party consumers (nvim plugins, TUIs, custom tooling).

FleetView is not an orchestrator. `orchestrate` decides what to run; FleetView exposes what's running.

FleetView is not a TUI in v1. A Ratatui dashboard is explicitly deferred to v2; see §13.

**Explicit non-goals for v1:**

- No Electron, Tauri, web frontend, or standalone interactive binary UI.
- No replacement for `psmux attach` — `fleet.attach` / `psmux fleet attach` returns a shell command; the user runs it.
- No new data stores — `state.json`, the backend JSON-RPC pipe, and the mycel bus are the only sources.
- No reimplementation of mycel; FleetView consumes its events.

## 2. Why now

The `ohboy-builds` branch has accumulated the full fleet substrate as primitives: `git worktree add` via `WorktreeSpec`, process isolation via the DAG runner, per-agent pane metadata, lifecycle events on the mycel bus, a typed JSON-RPC for inspection and control, and a crash-report subsystem wired into orchestrate worker state.

Running N processes in parallel on psmux today requires the operator to be a human window manager — four terminals for `ls`, `state.json` tailing, `attach`, and `git diff`. The primitives are clean; the aggregation is missing. FleetView fills that gap.

`oh-my-claudecode` (OMC, `psmux/oh-my-claudecode`, v4.13.1) demonstrates one rendering of the pattern via Claude Code status lines. FleetView ports OMC's hard-won pieces (awaiting-input detection, merge coordinator) onto psmux's stronger RPC, and exposes them as a substrate library with three frontends — not as a single LLM-facing tool.

## 3. Architecture

```
                 ┌──────────────────────────────────────┐
                 │   MCP client  │  human shell  │ lib  │
                 └───────┬───────────────┬────────┬─────┘
                         │               │        │
                   JSON-RPC stdio    argv + tty  Rust API
                         │               │        │
                 ┌───────┴───────┐ ┌─────┴────┐  │
                 │psmux-fleet-mcp│ │ psmux    │  │
                 │  (bin)        │ │  fleet   │  │
                 │               │ │ (subcmd) │  │
                 └───────┬───────┘ └─────┬────┘  │
                         └────────┬──────┘       │
                                  ▼              │
                          ┌──────────────────────┘
                          │     psmux-fleet (lib)
                          │  WorkspaceTable · hydration
                          │  detectors · merge-coordinator
                          │  state writer (CAS)
                          └──┬───────────┬───────────┬─────
                             ▼           ▼           ▼
                      orchestrate   backend pipe  mycel bus
                      state.json    JSON-RPC       subscribe
                      (CAS r/w)     (call+events)
                             │           │           │
                             └─────┬─────┴───────────┘
                                   ▼
                              git CLI
                        (diff · merge-tree · merge · worktree)
```

**Crate layout:**

- `crates/psmux-fleet/` — library. I/O scoped to exactly four sources: `state.json` (read + CAS write), backend JSON-RPC pipe (call + subscribe), mycel bus (subscribe, feature-gated), and the `git` CLI (shell-out). No database, no network calls, no additional filesystem paths outside `~/.psmux/fleet/`. Typed errors via `thiserror`.
- `crates/psmux-fleet-mcp/` — thin stdio MCP server binary. ~300 LOC of glue over the library.
- `src/fleet/` (inside existing `psmux` binary) — CLI subcommand tree. ~400 LOC of glue over the library.

**Stateless across invocations.** Every tool/command call re-hydrates from the sources of truth. No FleetView database, no long-lived state file, no daemon. Kill any frontend, restart it, the view is identical.

The one soft exception is `fleet.capture` pagination — `since_version` is client-retained state. Mitigated by an explicit `version_reset: true` flag in the response whenever the backend capture buffer rotates (ring-buffer wrap or pane respawn): the client discards its cursor and treats the payload as a fresh full snapshot. Documented on the tool in §5.3.

**Why a library, not a binary-only design?** Separating the core means:
- Three frontends share one implementation. Bug fixes land once.
- A future `psmux-fleet-tui` binary (v2) or `fleet.nvim` plugin re-uses the same core without touching any protocol.
- Unit tests exercise the core without standing up a process.

## 4. Data model

The unit of work is a **workspace**:

```rust
pub struct Workspace {
    pub id: String,                    // e.g. "fix-auth-bug"
    pub branch: Option<String>,        // e.g. "fleet/fix-auth-bug"
    pub worktree_path: Option<PathBuf>,
    pub pane_id: Option<String>,       // "%3"
    pub session: String,
    pub agent_kind: AgentKind,         // Claude | Codex | Pi | Shell | Unknown
    pub status: WorkspaceStatus,
    pub task: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub crash_dump_path: Option<PathBuf>,
    pub metadata: HashMap<String, String>, // @agent, @role, @color, ...
}

pub enum WorkspaceStatus {
    Pending, Running, Awaiting, Succeeded, Failed, Skipped,
}
```

A workspace maps 1:1 to an orchestrate worker *or* to a standalone `spawn_agent` call *or* to any pane tagged with an `@agent` metadata key. FleetView treats all three identically at the model layer; the difference is only in how the workspace was created.

**ID derivation:**
- Orchestrate worker → `worker_id` from plan.json.
- `fleet.new` spawn → `kebab(task) + "-" + base36(rand::random::<u32>())[:6]`. The random suffix gives ~32 bits of entropy so identical task strings produce distinct ids; pure `sha1(task)` was deterministic and would silently collide on duplicate tasks.
- Pre-existing pane with `@agent` metadata → value of `@agent`.
- Plain pane with none of the above → not surfaced by FleetView in v1. `psmux fleet ls` shows only workspaces with a discoverable identity.

**Status derivation (in priority order):**

1. If orchestrate has a terminal state in `state.json` → use it.
2. Else if the pane is dead (`pane_dead_time` set, or `context_exited` seen) → `Succeeded` if exit_code == 0, else `Failed`.
3. Else if awaiting-input detection fires (see §7) → `Awaiting`.
4. Else → `Running`.

`Pending` exists only for orchestrate-spawned workers blocked on a dependency edge, plus the brief reservation window used by `fleet.new` (§5.5) before orchestrate claims the slot. Single-worker `fleet.new` workspaces pass through `Pending` for milliseconds; a standalone `fleet.new` never stays `Pending` on a dependency.

## 5. Frontend surfaces

Eight verbs, mirrored across MCP and CLI. Each verb is one round trip.

| Verb | MCP tool | CLI |
|---|---|---|
| list | `fleet.list` | `psmux fleet ls` |
| status | `fleet.status` | `psmux fleet status <id>` |
| capture | `fleet.capture` | `psmux fleet tail <id>` |
| diff | `fleet.diff` | `psmux fleet diff <id>` |
| new | `fleet.new` | `psmux fleet new --task "..."` |
| merge | `fleet.merge` | `psmux fleet merge <id>` |
| kill | `fleet.kill` | `psmux fleet kill <id>` |
| attach | `fleet.attach` | `psmux fleet attach <id>` |
| prune | — | `psmux fleet prune` |

Below, each verb has params/returns and the drafted MCP `description` field. CLI flags mirror MCP params one-to-one (`--session`, `--status`, etc.). CLI output defaults to a human-readable table; `--json` emits the MCP response verbatim for scripting.

### 5.1 `fleet.list`

Lists workspaces in the session with status, id, task, agent, age.

**MCP description:** *"List all workspaces in the current psmux session. Returns status, id, task summary, agent kind, and age for each. Use this first when the user asks 'what's running' or 'show me the fleet'."*

**Params:** `session?: string`, `status?: enum`, `limit?: int = 50`
**Returns:** `{ workspaces: Workspace[], total: int }`

### 5.2 `fleet.status`

Detail view for one workspace.

**MCP description:** *"Get the full detail for one workspace by id, including the last 50 lines of its output (ANSI-stripped) and, if Awaiting, why. Use when the user asks about a specific agent or when deciding what to do next with one."*

**Params:** `id: string`
**Returns:** `Workspace` + `last_output: string` (last 50 lines of capture, ANSI-stripped) + `awaiting_reason?: string` (regex pattern or transcript-derived).

### 5.3 `fleet.capture`

Live pane output for a single workspace. Incremental via `since_version`.

**MCP description:** *"Return live output from a workspace's pane. Pass since_version to get only new lines since the last call. Output is ANSI-stripped by default. Max 500 lines per call — paginate for longer."*

**Params:** `id: string`, `since_version?: int`, `max_lines?: int = 500`
**Returns:** `{ text: string, data_version: int, ansi: bool, truncated: bool, version_reset: bool }`

`data_version` comes from the backend `capture` RPC's monotonic buffer version. If the backend does not expose one today, Commit 2 adds it; see §11.

**Buffer rotation:** when the backend ring-buffer wraps or the pane is respawned, `version_reset: true` is set and `text` contains the full current snapshot. Clients passing `since_version` older than the new baseline must discard their cursor and accept the reset payload.

### 5.4 `fleet.diff`

Git diff for a workspace's worktree.

**MCP description:** *"Show the git diff for a workspace's worktree against its base branch. Use mode='unified' for review, mode='files' for a summary, mode='log' for the commit list. Capped at 1000 lines — check the truncated flag."*

**Params:** `id: string`, `mode?: "unified" | "files" | "log" = "unified"`, `max_lines?: int = 1000`
**Returns:** `{ diff: string, base: string, head: string, file_count: int, insertions: int, deletions: int, truncated: bool }`

### 5.5 `fleet.new`

Create a workspace. Four steps, in order:

1. **Derive id and reserve slot.** Generate `id` per §4. Under a state.json CAS transaction: count active workspaces (Pending|Running|Awaiting), compare against `FLEET_MAX_CONCURRENT_WORKSPACES`, and if under cap atomically insert a `Pending` entry for the new id. This closes the TOCTOU window — two concurrent `fleet.new` calls at the cap cannot both pass, because the CAS transaction that inserts the reservation is what the next caller reads. CAS retry exhaustion or cap-hit surfaces as `QUOTA_EXCEEDED`.
2. **Write plan.json** to `~/.psmux/fleet/plans/<session>-<slug>.json`.
3. **Spawn `psmux orchestrate --plan <path>`** as a detached subprocess.
4. **Await ready signal**, then fill in `pane_id` and transition `Pending → Running` via CAS (orchestrate does this on worker start; FleetView only reads it).

**MCP description:** *"Spawn a new agent workspace for a task. The agent runs in its own git worktree and pane. Returns the workspace id immediately; pane_id may be null until orchestrate starts the pane — call fleet.status to confirm."*

**Params:** `task: string`, `agent?: AgentKind = Claude`, `base?: string = "origin/HEAD"`, `session?: string`, `force?: bool = false`
**Returns:** `{ id: string, pane_id?: string, plan_path: string }`

**Why subprocess + plan-on-disk, not in-process orchestrate call:** two explicit reasons, not accidental IPC.

1. **Survival across MCP-server death.** The orchestrate process must outlive whoever invoked it — MCP servers are per-client stdio processes that die when Claude Code exits; the CLI dies when its shell closes. A detached subprocess with `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` (no Job Object inheritance) is the only mechanism on Windows that guarantees the child outlives the parent.
2. **Durable plan for crash recovery.** plan.json on disk is orchestrate's existing restart-after-crash mechanism. Calling orchestrate in-process would mean keeping the plan in FleetView's memory; if anything crashed mid-run, the plan would be lost and the half-spawned workspace would be unrecoverable.

The file-round-trip cost is ~1 ms — negligible against the 200–2000 ms orchestrate cold-start on Windows.

**Windows subprocess flags:** `CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS`. Explicitly *not* inherited into the caller's Job Object.

**Ready signal:** Commit 3 subscribes to the mycel topic `psmux/orchestrate/worker/started` on a short-lived listener; the ready signal is the topic firing with a matching `worker_id`. Fallback: poll `state.json` every 100 ms for up to 10 s. Spawn latency instrumented via `fleet_spawn_ready_ms` log line.

**Concurrent-workspace quota:** `FLEET_MAX_CONCURRENT_WORKSPACES` env var (default `16`). Enforced via the step-1 CAS reservation above — not a separate pre-check. Override per-call with `force: true` param (MCP) / `--force` flag (CLI).

### 5.6 `fleet.merge`

Merge a completed workspace into its base branch.

**MCP description:** *"Merge a completed workspace into its base branch. MUST show the user fleet.diff output and get explicit confirmation before calling this. Returns conflicts on failure — does not attempt a merge with conflicts. Sequential: only one merge per base branch at a time."*

**Params:** `id: string`, `strategy?: "merge" | "squash" | "rebase" | "ff-only" = "merge"`

**Pre-flight:** `git merge-tree --write-tree` (Git 2.38+) for non-destructive conflict detection, ported from OMC's merge-coordinator. On older Git, falls back to a file-overlap heuristic.

**Concurrency:** acquires `.git/fleet.merge.<sanitized_base>.lock` (advisory exclusive file lock via `fs2::FileExt`) before pre-flight. `<sanitized_base>` replaces `/`, `\`, and `:` with `-` so base refs like `origin/main` map to `origin-main`. Another in-flight merge against the same base returns `{ ok: false, error: { code: "MERGE_IN_PROGRESS" } }`.

**On conflict:** Returns `{ ok: false, error: { code: "MERGE_CONFLICT", details: { kind, conflicts?: string[], stderr } } }`. No merge attempted. **Workspace status is unchanged** — if it was `Succeeded` it remains `Succeeded`, if it was `Running` it remains `Running`. FleetView and orchestrate never disagree about the workspace being in play after a failed merge attempt.

`details.kind` distinguishes failure modes so clients can react appropriately:
- `"text_conflict"` — real file overlap requiring manual resolution. `conflicts` lists paths.
- `"base_moved"` — `strategy: "ff-only"` requested but base has diverged. Client can retry with `"merge"` or `"rebase"`.
- `"merge_tree_unavailable"` — Git <2.38 fallback heuristic inconclusive. `stderr` carries git's own output.

**On clean:**
1. `git merge --no-ff <branch>` (or chosen strategy) against base.
2. `git worktree remove <path>`.
3. Kill pane via backend pipe.
4. Mark workspace `Succeeded` in `state.json` via the CAS-versioned atomic write (§6).
5. Reap the workspace's plan.json at `~/.psmux/fleet/plans/<session>-<slug>.json`.

**Returns:** `{ ok: true, commit: string }` or the error envelope above.

**Default strategy is `merge --no-ff`, not `squash`** — ported from OMC. Preserves integration history. Squash remains opt-in.

### 5.7 `fleet.kill`

Terminate a workspace's pane.

**MCP description:** *"Kill a workspace's pane. Pass cleanup=true to also remove its worktree, delete its branch, and reap its plan.json. Destructive — confirm with the user first when cleanup=true. Returns ALREADY_TERMINAL if the workspace already finished; call fleet.kill without effect rather than overwriting prior status."*

**Params:** `id: string`, `cleanup?: bool = false`, `force?: bool = false`
**Returns:** `{ ok: bool, cleaned: bool }`

**Terminal-state precedence.** If the workspace already has `finished_at` set (orchestrate recorded exit 0 or non-zero before this call), `fleet.kill` returns `ALREADY_TERMINAL` and does nothing. This prevents the write-partitioning edge case where orchestrate writes `Succeeded` on clean exit, then a late `fleet.kill` would clobber it to `Failed`. The operator should use `fleet.kill --cleanup` to reclaim resources without changing status in that case; the precedence rule allows it.

**Dirty-worktree handling when `cleanup=true`.** `git worktree remove` fails by default on uncommitted changes. FleetView does not auto-force this — data loss risk. If the worktree is dirty, returns `WORKTREE_DIRTY` with the changed paths in `details.dirty_paths`. Pass `force: true` to override (`git worktree remove --force`, explicitly documented as destructive in the MCP description above and the CLI's `--force` help text).

When `cleanup=true`, also reaps `~/.psmux/fleet/plans/<session>-<slug>.json`.

### 5.8 `fleet.attach`

Return a shell command for the user to attach into the pane. Not an automated attach — the frontend surfaces the command for the user to copy.

**MCP description:** *"Return the shell command the user can run to attach to a workspace's pane. Does not attach automatically. Show the user the command as a code block. If the workspace is still spawning, returns PANE_NOT_READY — wait and retry."*

**Params:** `id: string`
**Returns:** `{ args: string[], display_command: string, session: string, pane_id: string }`

`args` is the argv array (e.g., `["psmux", "attach", "-t", "demo", ";", "select-pane", "-t", "%3"]`). Callers must quote it for their own shell. `display_command` is a bash-syntax rendering for human display only (`psmux attach -t demo \; select-pane -t %3`) — MCP clients should show `display_command` to the user in a code block but invoke it via the user's own terminal, not reconstruct it from `args`.

**Error:** If the workspace has not yet been assigned a `pane_id` (spawn race from §5.5), returns `PANE_NOT_READY` — distinct from `PANE_DEAD` (not-yet-born vs. already-gone).

**CLI-only flag:** `psmux fleet attach <id> --exec` runs `args` directly via `std::process::Command::new(args[0]).args(&args[1..])`, bypassing any shell. CLI-only (MCP never auto-attaches — the user is not present at the MCP server's terminal).

### 5.9 `psmux fleet prune` (CLI-only)

Reap orphaned plan.json files under `~/.psmux/fleet/plans/` whose session no longer exists or whose workspace has reached a terminal state and been removed from state.json. Defensive: `fleet.merge` and `fleet.kill --cleanup` reap their own plans, but a crash between spawn and completion can leak one. No MCP equivalent — this is operational housekeeping, not agent-facing.

**Flags:** `--dry-run` lists what would be removed; `--older-than <duration>` gates on mtime (default: any orphan).
**Returns (with `--json`):** `{ removed: string[], kept: string[] }`.

## 6. State ownership & concurrency

`state.json` is co-owned by orchestrate and FleetView. Safety comes from three mechanisms together.

### 6.1 Versioned writes (CAS)

`state.json` gains a top-level `state_version: u64` field, monotonically incremented on every write. All writers — orchestrate and FleetView — use the same helper:

```rust
pub fn update_state<F>(path: &Path, mutate: F) -> Result<(), StateError>
where F: Fn(&mut State) -> Result<(), StateError>
{
    for _ in 0..3 {
        let (mut state, observed_version) = State::load(path)?;
        mutate(&mut state)?;
        state.state_version = observed_version + 1;
        if State::try_compare_and_swap(path, &state, observed_version)? {
            return Ok(());
        }
        // observed_version no longer current; reload and retry
    }
    Err(StateError::CasFailedAfterRetries)
}
```

`try_compare_and_swap` re-reads the on-disk `state_version` under the existing atomic `tempfile + rename` path. If it still matches `observed_version`, the rename proceeds; otherwise the write is abandoned and the caller retries.

Bounded retries (3, provisional) prevent live-lock. A CAS failure after three retries returns `STATE_VERSION_MISMATCH` to the caller. In steady state (write partitioning §6.2 in effect) three should be generous; under agent-finish storms with multiple MCP frontends plus orchestrate, contention is harder to predict. The retry helper increments a `fleet_state_cas_retries_total` counter (log-only in v1, wire to metrics in v2) so operators can see whether 3 is actually enough before the retry budget is treated as stable.

### 6.2 Write partitioning

Even with CAS, partitioning *who writes what* reduces contention:

- **Orchestrate owns:** all non-terminal transitions (`Pending → Running`, exit code recording, crash_dump_path assignment).
- **FleetView owns:** terminal transitions *after* exit_code is already set (`Running → Succeeded | Failed` post-merge; `Running → Failed` on explicit kill).

FleetView never writes non-terminal transitions. Orchestrate may still reach terminal states on its own (e.g., DAG completion) and will. This is safe: the last writer wins, both are writing the same terminal value.

### 6.3 Merge lock

Independent of state.json: `fleet.merge` takes a per-base-branch advisory file lock at `.git/fleet.merge.<sanitized_base>.lock` (see §5.6 for sanitization). Prevents concurrent `git merge` against the same base from multiple FleetView frontends (e.g., two Claude Code instances). Released in a `Drop` guard so process death doesn't leak it.

### 6.4 Backend pipe RPC extension (non-FleetView-internal change)

FleetView's `capture` flow (§5.3) depends on a monotonic buffer version that the backend `capture` RPC does not expose today. Commit 2 extends the backend pipe schema:

```
// Before:
response: { text: string, clean: bool }

// After:
response: { text: string, clean: bool, data_version: u64, version_reset: bool }
```

This is a **backwards-compatible addition**: existing backend clients that ignore unknown fields continue to work. Anyone *actively consuming* the `capture` RPC (Pi coding agent's `PsmuxAdapter`, Claude Code's TeammateTool) does not need to change, but will have access to the new fields if they opt in.

The backend pipe reference doc (`docs/custompane-backend.md`) is updated in Commit 2 alongside the code. This change is called out here because it is the one FleetView-driven change that touches a contract outside the fleet crates.

## 7. Session discovery

Resolution order for the target session, applied by every frontend:

1. **Explicit arg.** `--session <name>` on CLI, `session` param on MCP tool call. Always wins.
2. **Env var.** `$PSMUX_SESSION` — set automatically when the caller is inside a psmux pane. Used when no explicit arg.
3. **Auto-detect.** `psmux ls --json` — if exactly one session exists, use it. If zero, error `SESSION_NOT_FOUND`. If two or more, error `SESSION_AMBIGUOUS` with the list of candidates in `details.sessions`.

No silent defaults. No "first session alphabetically." Ambiguity surfaces.

**Deployment contract:** when running Claude Code outside psmux (common), the MCP config must include `--session` explicitly or the user must have exactly one live psmux session. This is documented in the install guide; the error message on `SESSION_AMBIGUOUS` tells the user exactly how to fix it.

## 8. Awaiting-input detection

Two paths, pluggable via an `AwaitingDetector` trait so future agent kinds register their own signals. **Regex is primary; the Claude JSONL transcript is a confidence booster, not the load-bearing path.** This inversion means a Claude Code release that changes the transcript format degrades `awaiting_reason` quality but does not break `Awaiting` detection itself.

### 8.1 Regex registry (primary, all agent kinds)

Pattern registry keyed by agent kind, seeded from OMC's production patterns (source file credits OMC):

```rust
static AWAITING_PATTERNS: &[(AgentKind, &[&str])] = &[
    (AgentKind::Claude, &[
        r"Do you want to proceed\?",
        r"\[y/n\]", r"\(y/N\)",
        r"Press Enter to continue",
        // + OMC's keyword-detector set (see commit d6354c8 for false-positive notes)
    ]),
    (AgentKind::Codex, &[ /* ... */ ]),
    (AgentKind::Shell, &[
        r"\$ $", r"PS .*> $", r"> $",
    ]),
];
```

Match runs against the last 10 lines of capture on every `data_version` bump, **debounced per workspace to at most 2 Hz** — a chatty agent producing hundreds of bumps per second does not amplify into hundreds of regex sweeps. First match wins. No match + recent output → `Running`. No match + 30s idle → still `Running` (not enough signal to call idle without ambiguity).

**False-positive protection:** OMC's `d6354c8` ("stop false-positive autopilot on 'autonomous'") documents real production breakage from over-eager keyword matching. Port that fix along with the patterns — case-sensitive, word-boundary anchors where appropriate, and never match on the word "autonomous" in prose.

### 8.2 Claude JSONL transcript (confidence booster, Claude agents only)

For panes identified as Claude agents (via `@agent` metadata or pane cwd), parse `~/.claude/projects/<proj>/<session>.jsonl`. If the last event is a `tool_use` with no matching `tool_result`, emit a high-confidence `awaiting_reason` like `"Tool approval: <tool_name>"`. This *refines* the regex-derived Awaiting signal with a precise reason; it never flips a `Running` workspace to `Awaiting` on its own.

**Coupling risk:** this path depends on Claude Code's private JSONL format, undocumented and subject to change. Behind a feature flag (`fleet-claude-transcript`, default on) so a broken Claude release can be disabled without recompiling. The pane-to-JSONL mapping uses the pane's cwd (matches `<proj>` in the path); when mapping or parse fails, the boost silently downgrades and the regex verdict stands. Flagged as a known risk (§12).

## 9. Error taxonomy

Uniform error envelope across MCP and CLI (CLI renders it as `error: <CODE>: <message>` on stderr with exit code 1):

```
{ ok: false, error: { code: string, message: string, details?: object } }
```

**Enumerated codes:**

| Code | When |
|---|---|
| `WORKSPACE_NOT_FOUND` | `id` does not match any known workspace |
| `SESSION_NOT_FOUND` | no psmux session exists at all |
| `SESSION_AMBIGUOUS` | multiple sessions and none specified |
| `PANE_DEAD` | workspace's pane is already gone |
| `PANE_NOT_READY` | workspace exists but `pane_id` not yet assigned (spawn race) |
| `ALREADY_TERMINAL` | `fleet.kill` on a workspace whose `finished_at` is set |
| `BACKEND_DOWN` | backend JSON-RPC pipe unreachable |
| `WORKTREE_DIRTY` | worktree has uncommitted changes at merge or cleanup-kill time |
| `MERGE_CONFLICT` | merge pre-flight or execution detected a conflict; see `details.kind` |
| `MERGE_IN_PROGRESS` | merge lock held by another frontend |
| `STATE_VERSION_MISMATCH` | CAS retries exhausted (bug signal or contention storm) |
| `GIT_UNAVAILABLE` | `git` binary not on PATH |
| `ORCHESTRATE_SPAWN_FAILED` | detached subprocess failed to start |
| `QUOTA_EXCEEDED` | `fleet.new` over concurrent-workspace cap (§5.5); pass `force` to override |

Tool/command schemas pinned via snapshot tests; adding a new error code is a non-breaking schema change, repurposing one is breaking.

## 10. Implementation plan

Four commits. Each independently mergeable.

### Commit 1 — crate scaffolding + core + `list`/`status`

- New workspace members: `crates/psmux-fleet/`, `crates/psmux-fleet-mcp/`.
- New subcommand group: `src/fleet/mod.rs` with `ls`, `status` dispatchers.
- `psmux-fleet`:
  - `Workspace`, `WorkspaceStatus`, `AgentKind` types.
  - `StateReader` + `StateWriter` with CAS.
  - `WorkspaceTable::hydrate_cold()`: `Vec<Workspace>` from state.json alone.
  - `AwaitingDetector` trait + `ClaudeTranscriptDetector`, `RegexDetector`.
  - Session resolver (§7).
  - Error enum with variants for all codes in §9.
- `psmux-fleet-mcp`: stdio MCP server skeleton, registers `fleet.list` and `fleet.status`.
- `psmux fleet`: CLI dispatcher for `ls` and `status`, `--json` flag.
- Dependencies: `rmcp` (primary; fallback = hand-rolled JSON-RPC stdio if immature), `serde`, `serde_json`, `regex`, `chrono`, `anyhow`, `thiserror`, `tokio`, `fs2`, `tempfile`, `rand` (id entropy per §4).
- Tests: fixture `state.json` + fixture capture text → hydration and awaiting detection are deterministic. CAS retry semantics under contention.

**Success criterion:** `psmux orchestrate examples/orchestrate/claude-agent-team.json && psmux fleet ls --json` prints a correct JSON table; `psmux-fleet-mcp --session demo` responds identically over stdio.

### Commit 2 — backend pipe client + live tools

- `psmux-fleet`:
  - `BackendClient`: named-pipe wrapper with `initialize`, `list`, `capture` long-poll.
  - Backend-pipe `capture` RPC extended to return `data_version: u64` if not already present. Schema change documented in backend pipe reference.
  - `MycelSubscriber` (feature-gated): optional consumer of `psmux/pane/*` and `psmux/exec/*`. When the `mycel` cargo feature is off, falls back to a two-layer poll: (a) `state.json` read every 500 ms (matches orchestrate's own poll cadence, so no extra latency for lifecycle transitions), and (b) backend-pipe `capture` long-poll with `since_version` for per-pane output changes (the long-poll itself carries the debounce; no extra timer needed). Awaiting detection runs on the `data_version` bump signal from layer (b), debounced at 2 Hz per §8.1 regardless of feature-gate state — so the polling fallback does not stack latency on top of the already-debounced detector.
  - `WorkspaceTable::hydrate_warm()`: merges state.json + backend + mycel.
- `psmux-fleet-mcp`: adds `fleet.capture`, `fleet.diff`, `fleet.kill`, `fleet.attach`.
- `psmux fleet`: adds `tail`, `diff`, `kill`, `attach` subcommands.
- Shell-out: `git diff` / `git log` via `std::process::Command`.
- Tests: scripted MCP client exercises all four new tools against a live orchestrate session.

**Success criterion:** a live session — "what's running, what's each agent doing, show me the diff from workspace X" — answerable entirely via MCP calls *or* entirely via `psmux fleet` commands.

### Commit 3 — `fleet.new`

- `psmux-fleet`:
  - `PlanBuilder`: writes one-worker plan.json to `~/.psmux/fleet/plans/`.
  - `SpawnService`: spawns `psmux orchestrate --plan <path>` with Windows-specific flags (§5.5).
  - Ready-signal path: mycel subscription on `psmux/orchestrate/worker/started` with 10s poll fallback; `fleet_spawn_ready_ms` instrumentation.
  - Concurrent-workspace quota check (`FLEET_MAX_CONCURRENT_WORKSPACES`, `force` override).
  - Slug derivation per §4 (`kebab + base36(rand)[:6]`).
- `psmux-fleet-mcp`: adds `fleet.new`.
- `psmux fleet`: adds `new` subcommand with `--force` flag.
- Tests: spawn a workspace, confirm `pane_id` populated via both mycel-fast-path and poll-fallback paths. Quota rejection test at cap; `--force` bypass test.

**Success criterion:** `psmux fleet new --task "fix login bug"` (or `fleet.new` over MCP) launches an agent in its own worktree and returns its id within 500 ms typical, 10 s worst case on Windows cold start.

### Commit 4 — `fleet.merge` + housekeeping

- `psmux-fleet`:
  - `MergeCoordinator`: ported from OMC — `git merge-tree --write-tree` pre-flight, `--no-ff` merge, abort-on-failure, sequential semantics. File-overlap heuristic fallback for Git <2.38.
  - Per-base file lock via `fs2`, sanitized path.
  - Terminal-state writer using CAS helper from Commit 1.
  - Plan.json reaping on `fleet.merge` success and `fleet.kill --cleanup`.
  - `PlanPruner`: orphan detection against `psmux ls` + `state.json`.
- `psmux-fleet-mcp`: adds `fleet.merge`.
- `psmux fleet`: adds `merge`, `attach --exec`, `prune`, and `install-mcp` subcommands. `install-mcp` writes the Claude Code MCP config snippet (§16 Q1) so first-time users don't hand-edit JSON.
- Tests: scripted integration test drives full lifecycle — spawn 2 agents, wait, merge one, kill one, destroy worktrees. Prune test with seeded orphan. Boundary-contract tests per §11.

**Success criterion:** a full demo flow — spawn 3 agents, watch progress, merge the green ones, destroy the red — completes with no terminal windows beyond the one the operator is in.

## 11. Testing

- **`psmux-fleet` unit tests:** hydration from fixture state.json (cold + warm), awaiting detector (both paths), merge-tree conflict detection with local git fixtures, CAS retry under contention, session resolver priority order.
- **MCP integration tests:** scripted MCP client (`mcp-test-client` or hand-rolled) exercises all 8 tools end-to-end against a real psmux session.
- **CLI integration tests:** `psmux fleet` subcommand tree exercised via PowerShell rails bench (like existing `tests/run_rails_bench.ps1`).
- **Contract tests (boundary contracts):** MCP tool schemas and CLI `--json` output schemas pinned via snapshot tests. Round-trip serialization of `Workspace`, error envelope. Coverage includes `version_reset` buffer-rotation semantics, `QUOTA_EXCEEDED` payload shape, and the `fleet_state_cas_retries_total` counter wiring. The [integration-smoke-test] skill generates these; see §15.
- **Regression fixtures from OMC:** port OMC's awaiting-input test corpus (redacted) to `psmux-fleet` to guard against regressions on the ported patterns.
- **Concurrency tests:** two writers racing on state.json with CAS must converge without data loss. Two merges against the same base must serialize.

## 12. Risks

| Risk | Mitigation |
|---|---|
| **state.json co-ownership.** Two writers racing. | CAS with `state_version` field (§6.1). Write partitioning (§6.2). Bounded retry. |
| **MCP tool response size.** `fleet.capture` can return huge text; `fleet.diff` on a big worktree can exceed 100 KB. | Cap `capture` at 500 lines per call, paginate via `since_version`. Cap `diff` at 1000 lines, surface a `truncated: true` flag. |
| **Awaiting-input false positives.** OMC shipped, regressed, and fixed false-positives (`d6354c8`). | Port the fix along with the patterns. Unit-test against OMC's known-bad corpus. Prefer transcript-based detection over regex for Claude agents. |
| **Autonomous merge without human review.** `fleet.merge` allows clean merges to proceed through a tool call. | MCP `description` on `fleet.merge` instructs clients to show `fleet.diff` output and get explicit user confirmation first. Behavioral contract, not server-side enforcement. |
| **Multi-frontend concurrent merge.** Two Claude Code instances, one operator running CLI — concurrent `git merge` on same base. | Per-base advisory file lock (§6.3). Fail fast with `MERGE_IN_PROGRESS`. |
| **Session ambiguity.** Multiple psmux sessions; frontend must know which. | Session resolver (§7) errors on ambiguity with actionable message. |
| **`rmcp` crate maturity.** The Rust MCP ecosystem is young. | Commit 1 includes a spike: if `rmcp` is unworkable, fall back to hand-rolled JSON-RPC stdio (~200 LOC). Library public API is unchanged either way. |
| **Git worktree quirks on Windows.** UNC paths, path-with-spaces. | Reuse orchestrate.rs's 8.3 short-path canonicalization. Shared helper in `psmux-fleet`. |
| **Pane-id instability.** `%N` ids change across session restarts. | Workspace keyed by stable `id`; `pane_id` resolved lazily per call. |
| **Claude JSONL format drift.** Transcript booster couples to Claude Code's private format. | Regex detector is primary (§8.1) — transcript drift degrades `awaiting_reason` quality but does not break Awaiting detection. Feature flag `fleet-claude-transcript` (default on) disables the booster without recompile. Covered in test suite with fixtures from current Claude versions. |
| **Detached subprocess survival on Windows.** `psmux orchestrate` launched by `fleet.new` must outlive caller. | Explicit `CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS` flags (§5.5); no Job Object inheritance. Tested by killing parent and observing child still runs. |

## 13. What's explicitly deferred to v2

- `psmux-fleet-tui` — Ratatui dashboard. Added only if v1 MCP + CLI coverage proves insufficient for parallel-agent visibility.
- psmux status-bar widget for ambient awareness ("3 running, 1 awaiting").
- Auto-merge on green test runs.
- Multi-repo workspaces (single-repo only in v1).
- Persistent merge audit log (`merges.jsonl`).
- `fleet.nvim` plugin.
- Web dashboard (read-only, for demos).
- Merge queues / rebase-onto-latest-base (`fleet.merge` in v1 merges against the recorded base at spawn time).
- **Proper metrics wiring.** v1 telemetry is **log-scraping only**: `fleet_spawn_ready_ms` (§5.5) and `fleet_state_cas_retries_total` (§6.1) are emitted as structured log lines, not Prometheus counters. Operators grep logs to tune the spawn-poll budget and verify CAS retries aren't pathological. v2 wires both into a proper metrics exporter.

## 14. Positioning

**One-line pitch:** *"FleetView is the aggregator over psmux's fleet primitives — one structured model of every workspace, with controlled merge/kill/spawn operations, rendered as CLI, MCP, or library depending on the caller."*

**Launch artifacts (two demos, side-by-side):**

1. **CLI demo** — one terminal, `psmux orchestrate plan.json`, then `psmux fleet ls --watch` showing four agents progressing; `psmux fleet diff w1 | less`; `psmux fleet merge w1`. Proves the substrate story without any LLM involvement.
2. **MCP demo** — 60-second Claude Code transcript. User: "spawn four agents to work on these four bugs." Claude: calls `fleet.new` ×4. User: "how are they doing?" Claude: calls `fleet.list`, renders a markdown table. User: "merge the green ones." Claude: calls `fleet.diff` for each, shows them, confirms, calls `fleet.merge`. Zero other terminal windows.

**Docs artifact:** `docs/fleet.md` with the MCP tool reference table, the eight tool schemas, the `psmux fleet` CLI reference, the Claude Code MCP config snippet, and one example of each demo. Link from README.

**Credit:** `psmux-fleet`'s awaiting-input detector and merge-coordinator are ported from `oh-my-claudecode`'s `src/team/` directory (Apache-2.0 / MIT compatible). Source-file headers credit the upstream project.

## 15. Integration with superpowers:integration-smoke-test

After this spec is approved, the `integration-smoke-test` skill generates a dedicated boundary-contract test module (`tests/fleet_contracts.rs`) covering:

- MCP tool request/response schema round-trips for all eight tools.
- CLI `--json` output matches MCP response shape exactly (fleet must be one model, two serializations).
- State envelope round-trip: `Workspace` serializes, deserializes, equals original.
- Error envelope: every code in §9 serializes to the documented shape.
- CAS semantics: two concurrent writers converge; three retries exhausted surfaces `STATE_VERSION_MISMATCH`.
- Session resolver: explicit arg beats env beats auto-detect; ambiguity errors.
- Awaiting detector contract: regex path alone produces `Awaiting` status; transcript booster refines `awaiting_reason` when available and silently degrades when the JSONL parse fails.
- Quota contract: 17th concurrent spawn at default cap returns `QUOTA_EXCEEDED` with `{ limit: 16, current: 16 }`; `force: true` bypasses.
- Buffer-rotation contract: forced backend pane respawn between two `fleet.capture` calls returns `version_reset: true` and a full snapshot.
- Plan reaping: post-merge, post-`kill --cleanup`, and `prune` all remove the expected plan.json files and nothing else.
- Spawn-race: `fleet.attach` against a workspace whose `pane_id` is still null returns `PANE_NOT_READY`, not `PANE_DEAD`.
- Terminal-state precedence: `fleet.kill` on a workspace with `finished_at` set returns `ALREADY_TERMINAL` and does not mutate status.
- TOCTOU quota: two concurrent `fleet.new` calls at cap − 1 — exactly one succeeds, the other gets `QUOTA_EXCEEDED`, never both.
- Merge conflict taxonomy: `details.kind` is one of `text_conflict | base_moved | merge_tree_unavailable` and matches the actual failure mode in fixture scenarios.
- Attach portability: `fleet.attach` returns `args: string[]` usable across shells; `--exec` invokes `args[0]` directly without a shell interpreter.

These tests are the standing boundary contracts. Any change that breaks them is a breaking change to a frontend API; schema version bumps as described in §11.

## 16. Open questions

1. **MCP auto-install.** The CLI ships with psmux; the MCP server is a separate binary. Claude Code users need a one-step install. **Resolved:** `psmux fleet install-mcp` subcommand lands in Commit 4 — it detects the user's Claude Code config path (`~/.config/claude/mcp_servers.json` on Linux/macOS, `%APPDATA%\Claude\...` on Windows) and appends the FleetView entry idempotently. No separate install script.
2. **`fleet.capture` vs `psmux attach`.** Claude can ask the user to attach for live output; `fleet.capture` is a snapshot. Answer: yes — Claude needs programmatic output for reasoning without human-in-the-loop attachment. The CLI version (`psmux fleet tail`) is also useful for scripting.
3. **CLI output format.** Default human-readable table; `--json` for machines. Column order, color usage, width adaptation: decide per-subcommand during Commit 1, document in `docs/fleet.md` as stable UI contract.
