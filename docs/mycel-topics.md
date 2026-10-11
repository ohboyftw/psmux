# Mycel Topic Schema — psmux

psmux publishes pane, exec, and session lifecycle events on the
[mycel](https://github.com/mycel-bus) pub/sub bus. The `mycel` cargo feature
that does this is on by default, and psmux hosts the bus itself (see
[The bus](#the-bus)). This document is the authoritative schema for every
`psmux/*` topic.

All payloads are JSON. `pane_id` is rendered as `%N` (e.g. `"%3"`), matching the
CLI's `-t %N` targeting. Every publish goes through
[`src/mycel.rs`](../src/mycel.rs) and is non-blocking — if the mycel server is
unreachable, the event is dropped silently.

---

## Pane lifecycle

### psmux/pane/created
**When:** a new pane is created — either via `new-window` / `new-session`
(first pane in a window) or `split-window` (additional pane in an existing
window).
**Payload:**
```json
{ "pane_id": "%3", "command": "pwsh" }
```
`command` is the spawn command string, or `""` when the pane runs the default
shell.
**Source:** `src/pane.rs:335` (new-window/new-session path), `src/pane.rs:922`
(split-window path).

### psmux/pane/ready
**Status:** implemented.
**When:** a pane is first ready by the `#{pane_ready}` rule: its reported
OSC 7501 state is `idle`, or, with nothing reported or only `done` and
`error` records left, it produced output and then was silent for 500 ms. The
same signal as the JSON-RPC `context_ready` push event
(`ready_signal: "reported"` or `"output_stable"`). This is not prompt
detection. Fires once per pane process: once after the pane is created and
once more after `respawn-pane`, never again when the pane later goes busy and
settles. To follow an agent turn by turn, subscribe to `psmux/pane/status`.
**Payload:**
```json
{ "pane_id": "%3", "elapsed_ms": 842 }
```
`elapsed_ms` is measured from pane creation to the moment the server saw the
pane settle.
**Source:** `src/server/mod.rs` server tick (emits
`LifecycleEvent::PaneReady` right after the `context_ready` RPC push),
published by `publish_lifecycle_to_mycel` in `src/control.rs`.

### psmux/pane/status
**Status:** implemented.
**When:** a pane's reported OSC 7501 agent state changes. The state is the
worst of all the pane's records (blocked > error > working > done > idle).
Only reported state counts: output on a pane that does not report never
publishes here, and a progress-only update is not a change. Mirrors the
JSON-RPC `state_changed` push event.
**Payload:**
```json
{ "pane_id": "%3", "state": "blocked", "previous_state": "working",
  "record_id": "t1", "kind": "permission", "app": "claude-code" }
```
`state` and `previous_state` are `idle`, `working`, `done`, `blocked`,
`error`, or `null` when the pane has no record (never `"clear"`). Unlike the
`state_changed` push, which omits them when empty, `record_id`, `kind` and
`app` are always present here and are `null` when empty. `title`, `msg` and
`progress` are never published.
The server samples the state once per 100 ms tick, so a state held for less
than one tick can be missed, and several changes within one tick publish as
one (or none, when the pane ends where it began). When a pane's process exits,
the final change is published before `psmux/pane/exited`; without
`remain-on-exit` a report written just before the exit can be lost.
**Source:** `emit_status_transition` in `src/server/helpers.rs` (server tick
and `prune_exited`), which emits `LifecycleEvent::PaneStatusChanged`,
published by `publish_lifecycle_to_mycel` in `src/control.rs`.

### psmux/pane/exited
**When:** the pane's child process has exited and the pane is being pruned
from the window tree. Replaces `psmux/pane/died` (see deprecation note below).
**Payload (current):**
```json
{ "pane_id": "%3", "exit_code": 0 }
```
**Payload (Phase 2 target, Task 11 enrichment):**
```json
{ "pane_id": "%3", "exit_code": 0, "elapsed_ms": 12480, "command": "cargo test" }
```
`exit_code` may be `null` if the process status could not be determined.
`elapsed_ms` + `command` will be added once the publish site is wired to the
same `ExitedPaneInfo` data used by the `context_exited` JSON-RPC push event.
**Source:** `src/tree.rs:563`.

### psmux/pane/died
**Status:** DEPRECATED — dual-published alongside `psmux/pane/exited` as a
one-release shim so existing subscribers (e.g. canopy listeners) don't break.
Will be removed in the release after Phase 2 ships. Use `psmux/pane/exited`
instead.
**When:** same trigger as `psmux/pane/exited` — pane process exit.
**Payload:** identical to the current `psmux/pane/exited` payload:
```json
{ "pane_id": "%3", "exit_code": 0 }
```
**Source:** `src/tree.rs:563`.

---

## Exec lifecycle

### psmux/exec/completed
**Status:** planned, landing in Phase 2 (Task 9).
**When:** a command launched via the JSON-RPC `exec` method (or the CLI mirror
`psmux exec -t %N -- <cmd>`) has finished. Mirrors the `exec_completed`
JSON-RPC push event.
**Payload:**
```json
{ "pane_id": "%3", "pid": 18472, "exit_code": 0, "elapsed_ms": 3412, "command": "cargo check" }
```
**Source:** `src/backend/dispatcher.rs` (co-publishes alongside the
`exec_completed` RPC push).

---

## Session lifecycle

### psmux/session/created
**Status:** planned, landing in Phase 2 (Task 10).
**When:** a new session is created via `new-session`.
**Payload:**
```json
{ "session_name": "work", "client_id": "psmux@hostname" }
```
`client_id` is the identifier passed to `MycelBus::new` at server startup.
**Source:** `src/server/mod.rs` (new-session handler).

### psmux/session/renamed
**Status:** planned, landing in Phase 2 (Task 10).
**When:** a session is renamed via `rename-session`.
**Payload:**
```json
{ "session_name": "work", "old_name": "scratch", "client_id": "psmux@hostname" }
```
**Source:** `src/server/mod.rs` (rename-session handler).

### psmux/session/killed
**Status:** planned, landing in Phase 2 (Task 10).
**When:** a session is killed via `kill-session` (or the last pane exits and
triggers session teardown).
**Payload:**
```json
{ "session_name": "work", "client_id": "psmux@hostname" }
```
**Source:** `src/server/mod.rs` (kill-session handler).

---

## Subscribing

Use the `mycel` CLI to subscribe to all psmux events at once:

```sh
mycel sub 'psmux/#'
```

Or scope to a specific category:

```sh
mycel sub 'psmux/pane/#'
mycel sub 'psmux/exec/#'
mycel sub 'psmux/session/#'
```

`+` matches one level and `#` matches any number of trailing levels (MQTT
style). `>` has no special meaning: `psmux/>` matches nothing.

---

## The bus

psmux carries its own mycel bus. The first time a session server publishes,
it looks for a live bus (the address in `~/.mycel/server`, then
`127.0.0.1:4321`). If none answers, it starts `psmux bus` in the background,
which listens on `127.0.0.1:4321` and writes `~/.mycel/server`. A standalone
`mycel` server that is already running is used instead.

The bus is its own `psmux.exe` process. It has no window, works in your home
directory, and keeps running after every session has ended, so
`kill-server` does not release `psmux.exe`. Stop it before you rebuild or
upgrade psmux, or the build or installer cannot replace the file:

```powershell
psmux bus stop        # or: mycel stop
```

`psmux bus stop` asks whatever bus answers (a psmux bus or a standalone
`mycel` server) to shut down. The bus is not gone for good: the next event
any running session publishes starts it again, within about 30 seconds. To
keep it stopped, set `PSMUX_BUS=0` before starting psmux. Session servers
read it once when they start, so servers already running (warm-pool ones
included) keep starting the bus until they exit.

Running `mycel start` while a psmux bus is up is not refused. On Windows the
standalone server's listener sets `SO_REUSEADDR` and binds the same port,
and the two servers then split clients between them. Run `psmux bus stop`
and set `PSMUX_BUS=0` first if you want the standalone server.

If `~/.mycel/server` cannot be written, the bus still runs and clients find
it on the port. After three starts in a row that never produced a live bus
(for example, another program holds port 4321), a session server waits 30
minutes before it tries again. The liveness probes that `psmux bus` and
`psmux bus stop` make connect as `psmux-bus-probe`, and the psmux bus
announces no `$mycel/clients/*` events for them.

| Variable | Effect |
|----------|--------|
| `PSMUX_BUS=0` | Never start a bus. An existing one is still used. Read when a session server starts. |
| `PSMUX_BUS_PORT` | Port to probe and to host on (default `4321`). |
| `PSMUX_BUS_DISCOVERY` | Discovery file to read and write (default `~/.mycel/server`). |

The `mycel` cargo feature is on by default. Build with
`--no-default-features` to compile the publishing out entirely.

Test scripts must not touch your real bus: `tests/_harness.ps1`,
`tests/run_all_tests.ps1` and the warm-pool tests set `PSMUX_BUS=0` and point
`PSMUX_BUS_PORT` and `PSMUX_BUS_DISCOVERY` at values that nothing uses.

---

See [`CLAUDE.md`](../CLAUDE.md) for the psmux architecture overview and the
Agent Execution Layer context that drives these events.
