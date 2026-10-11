# Improve Pi worker performance in psmux

Use this guide to reduce Pi worker startup cost, unnecessary context, and process overhead on the `ohboy-builds` branch. Keep psmux responsible for panes and process results. Load additional Pi orchestration only when a task needs it.

Reviewed against Pi 1.1.0 and psmux 3.4.0-ohboy. Check `pi --help`, `pi --version`, and `psmux --version` before using the commands with another release. The steps below are recommendations, not changes already applied to your installation.

## 1. Measure the delay you need to reduce

Separate these measurements before changing settings:

- Measure pane creation independently from shell readiness.
- Measure Pi startup until its first successful RPC `get_state` response.
- Measure the time from an accepted prompt to its first text output.
- Measure completion time and successful task outcomes.
- Record input tokens, cache-read tokens, peak memory, and process count.

Do not use `pi --version` as a startup benchmark. That command does not exercise a complete session. Use `pi --mode rpc --no-session` and send `get_state` instead. Close stdin after the test to request orderly shutdown.

Alternate the original configuration and the candidate configuration for at least three runs each. Keep the model, working directory, project-trust decision, and network mode the same. Record the first run separately because module and filesystem caches can change later results.

Use `--offline` to exclude automatic catalog and version network activity from a local startup test. That flag does not disable inference or every third-party extension's network calls. Disable extension-specific update checks separately.

## 2. Assign one owner to each orchestration responsibility

Choose the owners before starting workers:

- Use `psmux orchestrate` for a fixed plan with dependencies, pane creation, worktrees, and process exit results.
- Use CustomPaneBackend for reactive pane control and lifecycle events.
- Use ACP delegation for occasional tasks within one Pi conversation.
- Use Taskplane when the workflow needs managed coding batches, reviewers, merge agents, and recovery controls.
- Use Intercom when existing Pi sessions need to exchange messages.

Do not give two systems independent control of the same task queue, retry policy, or merge decision. A pane manager and a task supervisor can coexist if their responsibilities are explicit. See [Orchestrate](orchestrate.md) and [CustomPaneBackend](custompane-backend.md).

Start with one worker for small changes. Parallelize substantial, independent tasks rather than counting files or creating one agent per tiny edit. Assign one writable worktree to each implementation worker. Concurrent readers can share a repository, but they still need a resource limit.

## 3. Load only the resources a worker needs

Keep the supervisor's own extensions, such as Taskplane when it owns the coding batch, loaded in the supervisor session.

Test a smaller invocation for individual workers without disabling the supervisor's tools.

Use `--no-extensions` with explicit `-e` paths to choose the worker's executable extensions. Use `--no-skills` to omit the advertised skill catalog. Add back essential skills with `--skill`. Use `--no-prompt-templates` when the worker receives a complete task prompt rather than a workflow slash command.

Keep project instructions loaded. Do not use `--no-context-files` to hide build restrictions, safety requirements, or test commands.

Find installed extension paths with `pi list`. For a coding task that needs ACP but no web tools, use this PowerShell pattern:

```powershell
$acpEntry = 'C:\path\from\pi-list\billion-context-pi\dist\index.js'
$workerModel = 'provider/model-id'

pi --no-extensions -e $acpEntry `
	--no-skills --no-prompt-templates `
	--model $workerModel --thinking medium `
	--print '@task.md'
```

Replace both placeholders before running the command. Start Pi in the worker's assigned worktree. Add `-e` with the `pi-web-access` entry only when the task needs search or external sources.

If the worker's model comes from a provider that an extension registers, also load that extension with `-e`. Selecting the model without its extension does not restore the provider.

A direct extension-file load does not load that package's bundled skills or prompt templates. Add those explicitly when the task requires them.

For MCP tasks on Pi 1.1.0, native MCP is available without `pi-mcp-adapter`. A worker using `--no-extensions` must explicitly restore the required built-in extensions:

```text
-e builtin:mcp -e builtin:codemode -e builtin:tool-search
```

Review `mcp.json` before enabling servers. Prefer deferred or codemode tool exposure when a server provides many tools.

If the worker uses a fixed model, narrow its cycling scope with `--models provider/*`. Avoid inheriting patterns for virtual providers whose extensions are not loaded.

Do not assume `--no-tools` or an allowlist prevents extensions from starting processes. Those flags control model-facing tool selection, not extension lifecycle behavior.

## 4. Keep exactly one context manager

Use `billion-context-pi` as the sole context manager when you choose ACP. It cancels Pi's normal compaction through an event handler. Leave Pi's ordinary compaction setting enabled so removing ACP restores native behavior.

Use the package's documented global configuration at `~/.pi/acp.json`. Project overrides live at `<project>/.pi/acp.json`.

For externally scheduled leaf workers, consider this configuration:

```json
{
	"delegate": false,
	"autoUpdate": false
}
```

This prevents workers from offering a second delegation system and prevents package changes during a batch. It does not restrict tools that can launch processes through other paths.

If ACP owns delegation in an interactive supervisor session, use its separate delegation profile instead:

```json
{
	"autoUpdate": false,
	"delegate": {
		"enabled": true,
		"maxConcurrent": 2,
		"maxDepth": 1
	}
}
```

Treat two concurrent delegates as an initial experiment, not a measured optimum. ACP's `maxConcurrent` caps background delegates only. Synchronous calls bypass that cap, so the outer scheduler must also bound them. Do not apply both snippets globally at the same time. Choose the appropriate project or worker profile.

Keep ACP's context limit tied to verified model metadata. Do not shrink `modelContextLimit` merely to express a preferred context size. Use compression thresholds for that purpose.

Leave `rules` off until protected reminders prove useful. Keep enabled rules short because they occupy context that compression cannot reclaim. Consider the `lean` prompt pack only after testing whether the model still compresses and retrieves correctly.

Do not add another compressor. ACP detects common `bili` proxy configurations and stands down, but choose one compression path deliberately rather than relying on collision detection.

Test context-hook compatibility with every retained extension. Putting ACP last is not a complete solution: ACP rebuilds its view from the transcript, so an earlier extension's temporary message transformation can also be lost.

## 5. Remove shell and polling overhead from dispatch

For fixed plans, use `psmux orchestrate` rather than typing commands into shells and polling marker files. Orchestrate spawns each worker through `new-window --raw` and records the observed process exit code.

On Windows, avoid relying on a shell to resolve npm's `pi.cmd` or `pi.ps1` shims in a raw command vector. Resolve `node.exe` and the CLI path declared by the installed package's `bin.pi`, then launch Node directly. Pi 1.1.0 declares `dist/bundle/cli.js`. Use a plan command of this form:

```json
[
	"C:/path/to/node.exe",
	"C:/path/to/pi-coding-agent/dist/bundle/cli.js",
	"--model",
	"provider/model-id",
	"--print",
	"@task.md"
]
```

Resolve the paths on the target machine. Do not copy a user's installation paths into a shared plan. Use an explicit shell only when the command needs shell operators.

### Which completion signal each Pi mode gives

Pi 1.1.0 reports its state with OSC 7501 (the Program Status Protocol), but only in interactive mode. The three ways to run a worker give different signals:

| Pi mode | Completion signal | Agent state in psmux |
|---|---|---|
| `--print` (orchestrate plans) | Process exit code | None |
| `--mode rpc` | `agent_settled` event on stdout | None |
| Interactive (terminal UI in a pane) | OSC 7501 state | `#{pane_agent_state}`, `wait-pane --until`, `state_changed` |

In Pi 1.1.0 the program status reporter is built only by interactive mode. Print mode and RPC mode never send OSC 7501, and `PI_PROGRAM_STATUS=1` cannot change that. A `--print` worker in an orchestrate plan therefore has no `agent_state` in `state.json`, and orchestrate decides its result from the exit code alone. See [Orchestrate](orchestrate.md#agent-state).

Interactive Pi reports only after the terminal answers its support query (`OSC 7501;?`). psmux answers that query itself, so inside a psmux pane you do not need `PI_PROGRAM_STATUS=1`. Pi's own documentation says tmux and screen do not forward these reports; that does not apply to psmux, which reads them directly. One exception: a pane opened as a popup does not send the reply, so Pi in a popup never reports. Pi's states are:

- `working`: a run or compaction is in progress.
- `blocked`: an extension dialog or login waits for you.
- `done`: a run finished.
- `error`: a run ended with an error that Pi does not retry.
- `idle`: Pi started, or you cancelled the run.

### Wait for an interactive Pi run without polling

Name the pane by id. Take the id from `new-window -d -P -F '#{pane_id}'`, or from `pane_id` in orchestrate's `state.json`. From outside the pane's session, use `session:.%N`. `wait-pane --until` exits `2` for a `window.pane` index target.

`--until` is level-triggered and waits for one state. A pane that still shows `done` from its previous run satisfies `--until done` at once, so do not send a prompt and then wait for `done`. Wait in two steps instead:

```powershell
$pane = 'worker:.%7'
psmux send-keys -t $pane 'Fix the failing test in src/parser.rs' Enter

# 1. The run started. A short timeout catches a prompt that never started a run.
psmux wait-pane -t $pane --until working --timeout 30000
if ($LASTEXITCODE -ne 0) { throw "Pi did not start a run (rc $LASTEXITCODE)" }

# 2. The run stopped. A reported working or blocked state keeps this waiting;
#    done, error and a cancelled run (idle) let it return once the pane is quiet.
psmux wait-pane -t $pane --until idle --timeout 1800000

# 3. Read the outcome once.
psmux display-message -p -t $pane '#{pane_agent_state}'   # done, error or idle
```

Step 3 reads `done` for a finished run, `error` for a run that failed, and `idle` for a cancelled run, which is the pane-side match for `aborted: true` on `agent_settled`. Exit code `1` from step 2 means the timeout passed; exit code `2` means the pane died or is gone.

A run that stops for a dialog stays `blocked`, so step 2 keeps waiting. To act on that, run `psmux wait-pane -t $pane --until blocked` in a second waiter, or rely on the agent toasts, which are on by default (`set -g agent-toasts`; see [Agent state toasts](scripting.md#agent-state-toasts)). See [Waiting for an agent state](scripting.md#waiting-for-an-agent-state---until) for the full rules, including the 100 ms sampling interval.

### Reactive monitoring

For reactive monitoring, maintain one CustomPaneBackend connection. Use `context_exited` for pane lifecycle, `exec_completed` for command results, `state_changed` for OSC 7501 state changes, and versioned `capture` for fresh screen output. `state_changed` is pushed by the server, so the client does not ask again and again. See [CustomPaneBackend](custompane-backend.md#state_changed).

For a shell-based launcher that remains necessary, replace repeated `display-message` calls with one blocking wait. On this branch, `--timeout` is in milliseconds:

```powershell
psmux wait-pane -t 'worker:.%7' --ready --timeout 15000
```

`--ready` means the agent reported `idle`, or, when no agent reports `working` or `blocked`, that the pane's output has been quiet for 500 ms. Shell readiness does not prove that Pi has completed or accepted a task. Use `--until` for interactive Pi, the exit code for `--print`, and `agent_settled` for RPC workers.

Preserve real exit codes. A marker file means only that the wrapper wrote a file. When using JSON output, also inspect assistant errors and test results before accepting a task as successful. Include stderr and partial output in failure reports.

Orchestrate still polls worker exits internally. Moving an external polling loop into orchestrate does not eliminate all process creation. Measure that remaining cost before changing `src/orchestrate.rs`.

## 6. Reuse sessions only when the task benefits

For related steps, test a long-lived Pi RPC worker instead of restarting Pi after every small prompt. Wait for `agent_settled`, not `agent_end`, before dispatching the next step. Retries and queued work can continue after `agent_end`. In Pi 1.1.0, `agent_settled` carries `aborted`: `true` means the run ended because it was aborted, so treat it as not completed. If the `prompt` response reports `disposition: "handled"`, no run started, so do not wait for `agent_settled` at all.

Close the worker's stdin when the worker is no longer needed. Keep fresh sessions for unrelated tasks. Reuse can retain irrelevant history, extension state, and a worktree's assumptions.

Pin a suitable model per worker role before measuring routing. A model change can invalidate a cached conversation prefix. Use stronger reasoning for planning and difficult reviews, and test a lower level for bounded implementation tasks rather than lowering it everywhere.

Use cache warming only where the provider reports cache lifetimes and measured savings justify refresh requests. Idle warming is not useful for a one-shot process that exits immediately.

Keep complete logs in files. Send workers the relevant paths and excerpts rather than repeatedly sending full test output or captured terminal screens.

## 7. Bound concurrency and protect the host

Headless windows remove pane-size limits, not CPU, memory, or provider limits. Start with a small concurrency limit and measure successful throughput before raising it. At the reviewed head, orchestrate has no worker-concurrency field; group independent tasks into bounded waves or use a scheduler that supplies a cap.

Use a separate standard Windows account or an isolated VM for autonomous implementation on a machine that runs psmux sessions you depend on. Worktrees do not prevent a worker from terminating another process owned by the same user.

Never resolve a locked executable by stopping every process named `psmux`, `pmux`, or `tmux`. Give workers their own build output directory and stop only processes created by that run.

Run targeted tests during iteration. For this branch's unit targets, use:

```text
cargo test --lib --bin psmux
```

Inspect standalone integration targets before running them. `Cargo.toml` also declares memory and CPU detector tests. Run relevant integration tests in an isolated test session, then run the project's release checks under controlled conditions.

## 8. Accept a performance change only after these checks

- Confirm that startup, first text output, and task completion improved separately.
- Confirm that the same task still passes its objective checks.
- Confirm that provider failures and non-zero worker exits are reported as failures.
- Confirm that timeout cancellation stops the run's workers without affecting other sessions.
- Confirm that ACP can compress, search, decompress, resume, and fork with the retained extensions.
- Confirm that tool-call and tool-result pairing remains valid after compression.
- Confirm that every task's model, thinking level, workspace, and selected extensions appear in its run record.
- Confirm that cleanup leaves no abandoned workers, worktrees, or repeated monitoring processes.

## Related references

- [Pi packages](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/packages.md)
- [Pi settings](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/settings.md)
- [Pi RPC lifecycle](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/rpc.md)
- [Pi program status (OSC 7501)](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/terminal-setup.md#program-status)
- [Pi JSON events, including `agent_settled`](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/json.md)
- [Pi native MCP](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/mcp.md)
- [ACP repository and configuration reference](https://github.com/ranxianglei/billion-context-pi)
- [psmux orchestration](orchestrate.md)
- [psmux named-pipe control](custompane-backend.md)
- [psmux agent state waits and toasts](scripting.md#waiting-for-an-agent-state---until)
- [psmux shell setup](multi-shell.md)
