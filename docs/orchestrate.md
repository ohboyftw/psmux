# Orchestrate — DAG-driven worker panes

`psmux orchestrate <plan.json>` reads a worker DAG, provisions git worktrees
if requested, launches panes in topological order, polls for exit, and
skips dependents on failure. State is persisted to `state.json` alongside
the plan file for crash recovery and downstream CI consumption.

## Quick start

```powershell
psmux new-session -d -s build
psmux orchestrate ./plan.json --timeout 300000 --json
```

Exit codes:

| Code | Meaning |
|------|---------|
| 0 | All workers succeeded |
| 1 | At least one worker failed or was skipped |
| 3 | Wall-clock `--timeout` fired before all workers finished |

## `plan.json` schema

```json
{
  "version": 1,
  "session": "build",
  "workers": [
    {
      "id": "compile",
      "cwd": "./repo",
      "command": ["cargo", "build", "--release"],
      "depends_on": [],
      "env": { "RUSTFLAGS": "-C target-cpu=native" }
    },
    {
      "id": "test",
      "cwd": "./repo",
      "command": ["cargo", "test"],
      "depends_on": ["compile"]
    },
    {
      "id": "lint",
      "cwd": "./repo",
      "command": ["cmd", "/c", "cargo clippy -- -D warnings > clippy.log 2>&1"],
      "depends_on": []
    }
  ]
}
```

### Plan fields

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `version` | `1` | yes | Schema version; only `1` is accepted today. |
| `session` | string | yes | Session the worker windows live in. Must exist before `orchestrate` is called. |
| `workers` | array | yes | Non-empty list of worker definitions. |

### Worker fields

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `id` | string | yes | Unique within the plan. Used as window name and as the key in `state.json`. |
| `command` | string[] | yes | argv vector. Spawned **without shell wrapping** (equivalent to `new-window --raw`). Use `["cmd","/c","..."]` or `["bash","-c","..."]` when you need shell operators. |
| `cwd` | string | no | Working directory. Relative paths resolve against the plan file's directory. |
| `depends_on` | string[] | no | Ids of workers that must reach `Succeeded` before this one starts. Cycles fail validation. |
| `env` | object | no | Extra env vars. Merged on top of the inherited environment. |
| `worktree` | object | no | Create a git worktree at `cwd` before launching. See below. |

### `worktree` spec

```json
"worktree": {
  "repo": "./myrepo",
  "branch": "feature/x",
  "base": "origin/main"
}
```

`orchestrate` runs `git worktree add -b <branch> <cwd> <base>` before
spawning the worker. `--cleanup` reverses this (`git worktree remove`).

## Worker lifecycle

1. **Pending** — all workers start here.
2. **Running** — spawned into its own window via `new-window --raw`.
   Orchestrate sets `remain-on-exit on` on the session before the first
   spawn so dead panes persist long enough for their exit codes to be
   read.
3. **Succeeded** — `check_pane_exit` observed `pane_dead=1 pane_exit_code=0`.
   The preserved dead pane is then explicitly killed for cleanup.
4. **Failed** — any non-zero exit code, OR the pane vanished without an
   observable code (`exit_code = -1`, "pane gone"), OR the whole session
   was torn down (`exit_code = -3`, "session gone"), OR the `--timeout`
   fired (`exit_code = -2`, "orchestrate timed out").
5. **Skipped** — a dependency reached `Failed`; downstream workers
   never start.

## `state.json`

Written to `<plan_dir>/.orchestration/<session>/state.json`. Updated
after each worker status transition. The file survives `orchestrate`
crashing; subsequent re-runs resume from the persisted state.

```json
{
  "session": "build",
  "workers": {
    "compile": {
      "status": "succeeded",
      "pane_id": "%3",
      "exit_code": 0,
      "started_at": "2026-04-19T10:33:25.208Z",
      "finished_at": "2026-04-19T10:34:02.101Z"
    },
    "test": {
      "status": "failed",
      "pane_id": "%4",
      "exit_code": 101,
      "started_at": "2026-04-19T10:34:02.150Z",
      "finished_at": "2026-04-19T10:34:18.302Z",
      "crash_dump_path": null
    }
  }
}
```

When a worker's pane vanishes without an exit code (`exit_code = -1`),
orchestrate attempts to associate a psmux crash report from
`%LOCALAPPDATA%/psmux/crashes/` and records the path in
`crash_dump_path`. Session-gone (`-3`) does **not** get a crash lookup.

## Flags

| Flag | Purpose |
|------|---------|
| `--session <name>` | Override `plan.session`. Useful for parallel runs of the same plan. |
| `--cleanup` | Remove any worktrees the plan provisioned and delete `state.json`. |
| `--json` | Emit the final `state.json` to stdout instead of the human summary. |
| `--timeout <ms>` | Hard wall-clock ceiling. Running/pending workers get `exit_code = -2` when it fires. Exit code `3`. |

## Worker command semantics

All `command` vectors are spawned directly — no shell wrapper. This
gives predictable behavior across pwsh/bash/cmd hosts. Pick the shell
explicitly when you need its operators:

```json
// pwsh (Windows default, but explicit):
"command": ["pwsh", "-NoProfile", "-Command", "Get-Process | Sort CPU | Select -First 5"]

// bash (Git Bash / WSL):
"command": ["bash", "-c", "set -euo pipefail; ./build.sh | tee build.log"]

// cmd (fastest for trivial Windows ops):
"command": ["cmd", "/c", "copy /Y src dest && echo done"]
```

The `orchestrate` invocation and the worker panes inherit `PSMUX_SESSION`
and `PSMUX=1` — use them inside workers to identify the parent server
and guard against nested psmux invocations.

## Recovery

If `orchestrate` is killed mid-run:

1. Re-run with the same plan and session. It reads `state.json` and
   resumes from where it left off; workers already in a terminal state
   are not re-spawned.
2. To start fresh, pass `--cleanup` first to purge worktrees and state.

## Known limitations

- Workers see the pane tree of the parent session — they can run any
  psmux command, including `kill-session`. Orchestrate has no
  sandboxing. Don't run untrusted plans.
- `--timeout` is wall-clock, not per-worker. There is no per-worker
  timeout today; encode it inside the command (e.g. `timeout /t 60 /nobreak`).
- Dependency edges are all "succeeded-only" — there is no "run-on-failure"
  edge type.
