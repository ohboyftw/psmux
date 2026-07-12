# Orchestrate examples

Ready-to-run `plan.json` files for `psmux orchestrate`. Each demonstrates a
different archetype. See [docs/orchestrate.md](../../docs/orchestrate.md) for
the full schema and CLI reference.

## Running an example

Every plan requires its target `session` to exist before orchestrate is called:

```powershell
psmux new-session -d -s orc-hello
psmux orchestrate examples/orchestrate/hello-world.json --timeout 30000 --json
```

Each worker gets its own window inside the session. `--timeout` bounds total
runtime (milliseconds); `--json` emits the final `state.json` to stdout.

Cleanup (removes worktrees if the plan provisioned any, plus `state.json`):

```powershell
psmux orchestrate examples/orchestrate/hello-world.json --cleanup
psmux kill-session -t orc-hello
```

## The plans

### `hello-world.json`

Smoke test — three trivial `cmd /c echo` workers. `greeter` runs first,
`responder` depends on it, `independent` runs in parallel with `greeter`.
Proves the DAG scheduler and the `--raw` argv path in under a second.

No external tools required beyond `cmd.exe`.

### `cargo-pipeline.json`

Traditional build fan-out: `compile` → `test` (sequential), plus `lint` and
`fmt-check` in parallel. The kind of thing CI would do on a push, minus the
CI server.

Requirements: `cargo` on PATH. Run from a Rust project root. Exit code is 0
if every worker succeeds, 1 if any fails (including lint/fmt).

### `claude-swarm-demo.json` — the durable, send-keys-free swarm pattern

Two `claude -p` workers in parallel, each in its own git worktree, plus a
dependent `assemble` step. Every worker is spawned **argv-direct** via
`new-window --raw` — no typing into a shell, no readiness race. Each poet
**writes and commits** its haiku to its worktree branch; `assemble` collects
the results with `git show` (git is the reliable artifact channel — see
caveat 3). The scheduler reads each worker's **exit code** from `state.json`.

```powershell
psmux new-session -d -s orc-claude-demo
psmux orchestrate examples/orchestrate/claude-swarm-demo.json --timeout 180000 --json
type examples/orchestrate/combined-haiku.txt
psmux orchestrate examples/orchestrate/claude-swarm-demo.json --cleanup
psmux kill-session -t orc-claude-demo
```

**What this proves:** the two poets spawn argv-direct with **zero
`send-keys`**, run **in parallel** (start within ~0.1 s of each other), and
`assemble` **waits for both** before running (and is *skipped* if either
fails). `state.json` is the machine-readable proof.

**Requirements:** `claude` on PATH (`claude -p` headless). The poets use
`--dangerously-skip-permissions` so the headless agent can run `git commit`
without a prompt — acceptable here because each runs in a throwaway worktree.

**Windows notes (from validating this plan):**

- **Collect via git commits, not stdout.** Headless `claude -p ... > file`
  inside a pane exits 0 but leaves no file — its stdout doesn't survive the
  pane redirect. So each poet commits, and `assemble` reads with
  `git -C <worktree> show HEAD:haiku.txt`. (For pure command output, the
  backend **`exec` RPC** returns `{exit_code, stdout, ...}` directly.)
- **Never use a bare `bash` worker.** On Windows, raw `bash` resolves to
  **WSL** bash (no Windows `claude`, slow init → exit 127). Use `cmd` or an
  explicit `"C:/Program Files/Git/bin/bash.exe"` argv[0], as `assemble` does.
- **Worktree path doubling — fixed.** `orchestrate` now resolves the plan
  directory to an absolute path, so invoking from any directory no longer
  doubles the worktree path. (Was `.../examples/orchestrate/examples/
  orchestrate/worktrees/...` when run from the repo root.)
- **Concurrent `claude -p` can transiently contend** on claude's own state
  lock ("file is being used by another process"). Rare; re-run, or stagger
  the poets with a dependency edge if it recurs. This is a claude-side lock,
  not an orchestrate worktree race (provisioning is already sequential).

### `claude-agent-team.json` — illustrative (edit before running)

Three Claude Code agents coordinating on a refactor:

- `researcher` — summarises a module into a markdown doc
- `coder` — (depends on researcher) applies the recommended refactor
- `reviewer` — (depends on coder) writes a diff review

Each agent runs in its own git worktree, so their edits never clobber each
other. `base` chains the worktrees: researcher branches from `ohboy-builds`,
coder from `agent/researcher`, reviewer from `agent/coder`. When every
worker exits, the three branches form a clean review chain you can diff
with `git log agent/reviewer --oneline`.

Requirements: `claude` CLI on PATH; run from a git repo; worker prompts
assume the repo has an `auth` module — edit for your target before running.

## Writing your own

Copy any of the three as a starting point. The [plan.json schema in
docs/orchestrate.md](../../docs/orchestrate.md#planjson-schema) lists every
field. Key things to remember:

- `command` is argv — spawn-direct, no shell wrap. Use `["cmd","/c","..."]`
  or `["bash","-c","..."]` when you need shell operators.
- `cwd` paths resolve against the plan file's directory.
- `depends_on` ids must match another worker's `id`. Cycles fail validation
  at parse time.
- `worktree` auto-runs `git worktree add -b <branch> <cwd> <base>`;
  `--cleanup` runs `git worktree remove` in reverse.
