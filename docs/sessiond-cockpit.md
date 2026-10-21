# sessiond cockpit feed

psmux can show the state of agent sessions running on another machine under
sessiond, in its status line. It reads sessiond's state topic
(`sessiond/session/state` and `sessiond/session/removed`) as JSON lines from a
command you choose.

It is off unless you set `@sessiond-feed`.

## Turn it on

```tmux
set -g @sessiond-feed "ssh sessiond-box tail -n +1 -F ~/.sessiond/state.jsonl"
set -g status-right "#{sessiond_summary} | %H:%M"
```

psmux starts the command the first time a `#{sessiond_*}` variable is expanded,
and restarts it with backoff (5 s, doubling to 5 min) if it exits. The command
runs through `powershell -NoProfile -Command` on Windows and `sh -c` elsewhere.

The command must replay current state when it starts, because psmux clears its
board on every (re)start. `tail -n +1` replays the whole file; `tail -n 0` would
show nothing until the next change.

The command is read once per server: changing `@sessiond-feed` takes effect
when the server restarts.

## Format variables

| Variable | Value |
|---|---|
| `#{sessiond_summary}` | non-zero counts, most urgent first, e.g. `B1 W2 D3` (blocked, error, working, done, idle) |
| `#{sessiond_blocked}`, `#{sessiond_error}`, `#{sessiond_working}`, `#{sessiond_done}`, `#{sessiond_idle}` | sessions in that state |
| `#{sessiond_total}` | sessions known |
| `#{sessiond_feed}` | `off` (option unset), `live` (command running), `down` (command exited, waiting to restart) |

All are empty while the feed is off. The status line picks up changes on its
next refresh (`status-interval`).

## Input format

One envelope per line, as sessiond's JSONL publisher writes them:

```json
{"topic": "sessiond/session/state", "published_at": "2026-10-21T20:41:07.901Z", "payload": {"session": "3f6c2a0e-...", "state": "blocked", "since": "2026-10-21T20:41:07.512Z", "reason": "pending:command"}}
```

Lines that are not valid JSON, are on another topic, or carry a state outside
`working`, `blocked`, `done`, `idle`, `error` are ignored. Unknown payload keys
are ignored.
