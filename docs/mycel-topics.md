# Mycel Topic Schema — psmux

psmux publishes pane, exec, and session lifecycle events on the
[mycel](https://github.com/mycel-bus) pub/sub bus. The `mycel` cargo feature
that does this is on by default, and psmux hosts the bus itself (see
[The bus](#the-bus)). This document is the authoritative schema for every
`psmux/*` topic.

All payloads are JSON. `pane_id` is rendered as `%N` (e.g. `"%3"`), matching the
CLI's `-t %N` targeting. Every event is published from one function,
`publish_lifecycle_to_mycel` in [`src/control.rs`](../src/control.rs), which
`emit_lifecycle` calls from the trigger sites listed below. Publishing never
blocks a session server: if no bus answers and none can be started, the event
is dropped.

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
**Trigger:** `src/pane.rs` (the new-window/new-session path and the
split-window path).

### psmux/pane/ready
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
**Trigger:** `src/server/mod.rs` (the server tick, where `context_ready` is
pushed).

### psmux/pane/status
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
**Trigger:** `src/server/helpers.rs` (`emit_status_transition`, from the
server tick and `prune_exited`).

### psmux/pane/exited
**When:** the pane's child process has exited and the pane is being pruned
from the window tree. Replaces `psmux/pane/died` (see deprecation note below).
**Payload:**
```json
{ "pane_id": "%3", "exit_code": 0 }
```
`exit_code` may be `null` if the process status could not be determined.
Adding `elapsed_ms` and `command` (as the `context_exited` JSON-RPC push event
carries) is planned, not done.
**Trigger:** `src/tree.rs` (pane pruning).

### psmux/pane/died
**Status:** DEPRECATED — published right after every `psmux/pane/exited` as a
one-release shim so existing subscribers (e.g. canopy listeners) don't break.
Use `psmux/pane/exited` instead.
**When:** same trigger as `psmux/pane/exited`.
**Payload:** identical to `psmux/pane/exited`:
```json
{ "pane_id": "%3", "exit_code": 0 }
```

---

## Exec lifecycle

### psmux/exec/completed
**When:** a command launched via the JSON-RPC `exec` method (or the CLI mirror
`psmux exec -t %N -- <cmd>`) has finished, alongside the `exec_completed`
JSON-RPC push event.
**Payload:**
```json
{ "pane_id": "%3", "pid": 0, "exit_code": 0, "elapsed_ms": 3412, "command": "cargo check" }
```
`pane_id` is the exec context id. `pid` is always `0` for now.
**Trigger:** `src/backend/dispatcher.rs` (the dispatcher thread).

---

## Session lifecycle

`client_id` is `psmux@<COMPUTERNAME>`, the id the session server connects to
the bus with. It is also the `publisher` of every psmux event.

### psmux/session/created
**When:** a session server starts a new session. With the warm pool on (the
default), `new-session` usually claims a standby server that started earlier
as `__warm__` (`__warm__0`, `__warm__1`, ... for a larger pool), so the
session first appears here under that name, followed by
`psmux/session/renamed` with the claimed name when it is claimed. A standby
that is never claimed is never renamed.
**Payload:**
```json
{ "session_name": "work", "client_id": "psmux@hostname" }
```
**Trigger:** `src/server/mod.rs` (server start).

### psmux/session/renamed
**When:** a session is renamed via `rename-session`, or a warm standby is
claimed by `new-session` (`old_name` is then `__warm__` or `__warm__N`).
**Payload:**
```json
{ "session_name": "work", "old_name": "scratch", "client_id": "psmux@hostname" }
```
**Trigger:** `src/server/mod.rs` (`rename_server_session`, from the
rename-session and claim-session handlers).

### psmux/session/killed
**When:** a session is killed via `kill-session`.
**Payload:**
```json
{ "session_name": "work", "client_id": "psmux@hostname" }
```
**Trigger:** `src/server/mod.rs` (kill-session handler).

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

### Topic rules on the psmux bus

The psmux bus speaks the same wire protocol as the standalone C++ `mycel`
server, so `mycel` CLI, Python, Rust and MCP clients work against it
unchanged. A few rules are stricter or fixed:

- A wildcard must be a whole level and `#` must be the last level. A
  Subscribe or Unsubscribe with a pattern such as `a+` or `a/#/b` gets an
  `Error` reply instead of an `Ack`. A Publish to a topic containing `+` or
  `#` is dropped.
- A wildcard in the first level never matches a topic that starts with `$`,
  as in MQTT. That covers `+/...` as well as `#`, and replay as well as live
  delivery, so `mycel replay '#'` no longer returns `$mycel/*` heartbeats.
  Subscribe to `$mycel/#` to see them.
- Unsubscribe removes only the named pattern (the C++ server removed all of
  the connection's subscriptions).
- Each connection queues at most 1000 live events. When a subscriber falls
  behind, the oldest queued events are dropped and counted in the status
  reply (`ClientInfo` field 7, `events_dropped`). Acks, status replies and
  replay batches are never dropped.
- A Replay followed by a Subscribe of the same pattern also delivers the
  events published between the two, so `mycel sub --replay` no longer loses
  them.
- Replay history keeps up to 10 000 events and 64 MiB per topic, 256 MiB in
  all, for 10 minutes; past a limit the oldest events go first (an event's
  size is its encoded size plus 128 bytes). A topic whose events have all
  aged out is forgotten within one heartbeat (30 s), even if nobody asks for
  a replay or status. Sequence numbers never repeat within a topic: a topic
  published again after it was forgotten continues above the highest number
  the bus has handed out instead of restarting at 1.

---

## The bus

psmux carries its own mycel bus. The first time a session server publishes,
it looks for a live bus (the address in `~/.mycel/server`, then
`127.0.0.1:4321`). If none answers, it starts `psmux bus` in the background,
which listens on `127.0.0.1:4321` and writes `~/.mycel/server`. A standalone
`mycel` server that is already running is used instead.

A bus is live only if it answers a Hello. A `~/.mycel/server` whose address
does not answer (left behind by a crash, for example) is stale: it is ignored
and the next bus to start overwrites it. `~/.mycel/server` means
`%USERPROFILE%\.mycel\server`, falling back to `$HOME`, as every mycel client
reads it. A psmux bus binds `127.0.0.1:4321` without `SO_REUSEADDR`, writes
the file only after the bind succeeds, and removes it when it stops cleanly.

The bus is its own `psmux.exe` process. It has no window, works in your home
directory, and keeps running after every session has ended. It is shared by
every session server on the desktop and by other mycel clients, so
`kill-server` leaves it running: the sweep that ends leftover psmux
processes skips the bus that `~/.mycel/server` names (by its `pid`, while it
still holds its port). Stop it before you rebuild or upgrade psmux, or the
build or installer cannot replace the file:

```powershell
psmux bus stop        # or: mycel stop
```

`psmux bus stop` stops only a bus that psmux started: `~/.mycel/server`
must name its `pid`, that pid must be a running psmux, and the bus at the
file's address must answer. A standalone `mycel` server (it writes no `pid`)
or a bus it cannot verify is left running, and the command says so and
exits non-zero; use `mycel stop` for those. The bus is not gone for good:
the next event any running session publishes starts it again, within about
30 seconds. To
keep it stopped, set `PSMUX_BUS=0` before starting psmux: that turns
publishing off, so session servers neither start a bus nor publish to one
that is running. Session servers read it once when they start, so servers
already running (warm-pool ones included) keep publishing, and starting the
bus, until they exit.

Running `mycel start` while a psmux bus is up is not refused. On Windows the
standalone server's listener sets `SO_REUSEADDR` and binds the same port,
and the two servers then split clients between them. To use the standalone
server instead, run `psmux bus stop` and then `mycel start` straight away:
session servers find the live standalone server and publish to it rather
than start their own bus. (`PSMUX_BUS=0` would turn psmux's publishing off
altogether.)

If `~/.mycel/server` cannot be written, the bus still runs and clients find
it on the port, but `psmux bus stop` cannot verify it; stop that one with
`mycel stop`. After three starts in a row that never produced a live bus
(for example, another program holds port 4321), a session server waits 30
minutes before it tries again. The liveness probes that `psmux bus` and
`psmux bus stop` make connect as `psmux-bus-probe`, and the psmux bus
announces no `$mycel/clients/*` events for them.

| Variable | Effect |
|----------|--------|
| `PSMUX_BUS=0` | Publishing off: never start a bus and never publish, not even to a bus that is already running. Read when a session server starts. |
| `PSMUX_BUS_PORT` | Port to probe and to host on (default `4321`). |
| `PSMUX_BUS_DISCOVERY` | Discovery file to read and write (default `~/.mycel/server`). |

The `mycel` cargo feature is on by default. Build with
`--no-default-features` to compile the publishing out entirely.

Test scripts must not touch your real bus: `tests/_harness.ps1`,
`tests/run_all_tests.ps1` and the warm-pool tests set `PSMUX_BUS=0` and point
`PSMUX_BUS_PORT` and `PSMUX_BUS_DISCOVERY` at values that nothing uses. That
alone is not enough while the warm pool is on: `new-session` claims a standby
that read its environment when it was spawned (from your session, so it
publishes to your bus), and a standby a test spawns hands the test's
environment to your next session. So the harness and `run_all_tests.ps1`
also set `PSMUX_NO_WARM=1`, and the two warm-pool tests, which need the
pool, run in a scratch `PSMUX_DATA_DIR` where they can claim only their own
standbys.

`scripts/mycel_bus_conformance.ps1` checks a freshly built `psmux bus`
against the real Python mycel client. It runs the bus on a free port with a
temporary `USERPROFILE`, so it never touches your `~/.mycel/server` or the
bus on 4321, and it stops only the bus process it started.

---

See [`CLAUDE.md`](../CLAUDE.md) for the psmux architecture overview and the
Agent Execution Layer context that drives these events.
