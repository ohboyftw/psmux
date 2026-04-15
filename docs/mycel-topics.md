# Mycel Topic Schema — psmux

psmux publishes pane, exec, and session lifecycle events on the
[mycel](https://github.com/mycel-bus) pub/sub bus when built with the optional
`mycel` cargo feature. This document is the authoritative schema for every
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
**Status:** planned, landing in Phase 2 (Task 8).
**When:** a pane reaches the idle shell prompt — mirrors the JSON-RPC
`context_ready` push event. Fires once per pane, after first prompt detection.
**Payload:**
```json
{ "pane_id": "%3", "elapsed_ms": 842 }
```
`elapsed_ms` is measured from pane creation to prompt-ready.
**Source:** `src/backend/dispatcher.rs` (co-publishes alongside the
`context_ready` RPC push).

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
mycel sub 'psmux/>'
```

Or scope to a specific category:

```sh
mycel sub 'psmux/pane/>'
mycel sub 'psmux/exec/>'
mycel sub 'psmux/session/>'
```

The `>` wildcard matches any number of trailing segments (NATS-style).

---

## Feature flag

Mycel integration is gated behind the `mycel` cargo feature:

```sh
cargo build --release --features mycel
```

Without the feature, `publish_pane_event` calls are compiled out entirely — no
runtime cost, no dependency on `mycel-client`. The published crates on
crates.io do not enable `mycel` by default.

---

See [`CLAUDE.md`](../CLAUDE.md) for the psmux architecture overview and the
Agent Execution Layer context that drives these events.
