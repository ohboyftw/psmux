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

### `claude-agent-team.json`

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
