# Trace-Debug Analysis — sync/rails-p0-p2

Per-port trace checklist for the rails sync batch.  Each entry names the
**invariant**, the **trace points** to watch, and the **expected trace
pattern** when the fix fires.  Run this when validating a release build
against a live session — attach excerpts to the PR description.

How to run:

```powershell
$env:RUST_LOG = "psmux=trace"
pwsh scripts/install.ps1 -NoTools -Force
psmux new-session -s rails-verify -d
# ... exercise each scenario below, capturing the trace excerpts
```

Trace-level logs are emitted by `log::trace!` calls in the source.  If a
trace point isn't yet instrumented, add a single-line `log::trace!` at
the boundary the fix touches, then re-run.

---

## P0.1 — allow_set_title + OSC 0/2 title propagation

**Invariant:** when `allow-set-title on` and `title_locked=false`, an OSC
0/2 sequence from the child must update `Pane.title` by the next
DumpState / ListPanes / DisplayMessage tick.

**Trace points:**
- `src/server/helpers.rs::propagate_osc_titles` — log the pane count walked and
  any title change
- `src/server/helpers.rs::propagate_osc_titles_in_tree` Leaf branch — log
  pane.id, old title, new OSC title, title_locked
- Any CtrlReq arm that now calls propagate_osc_titles (DumpState, ListPanes,
  ListWindows*, DisplayMessage, ListPanesJson, ListWindowsJson)

**Expected trace pattern (allow-set-title on, OSC received):**
```
TRACE propagate_osc_titles: walking 1 window(s)
TRACE leaf id=%1 title="pane %1" locked=false osc="my-custom-title"
TRACE leaf id=%1 applied title="my-custom-title"
```

**Expected trace pattern (allow-set-title off):**
```
TRACE propagate_osc_titles: short-circuit (allow_set_title=false)
```

**Verify:**
```powershell
psmux set -g allow-set-title on
psmux send-keys -t %1 'Write-Host "`e]2;verified-title`a"' Enter
psmux wait-for -t %1 --output "verified-title" --timeout 5000
psmux display-message -p '#{pane_title}'   # must print: verified-title

psmux select-pane -t %1 -T locked
psmux send-keys -t %1 'Write-Host "`e]2;should-not-apply`a"' Enter
Start-Sleep -Milliseconds 500
psmux display-message -p '#{pane_title}'   # must still print: locked
```

---

## P0.2 — set-option -o via user_set_options HashSet

**Invariant:** `set-option -o key value` is a no-op after any prior
`set-option key ...` on the same key, regardless of the option's default
value.  `set-option -u key` removes the key from the tracking set.

**Trace points:**
- `src/config.rs::parse_set_option` — log key, only_if_unset, unset_mode,
  HashSet membership at decision time, and the write-vs-noop outcome

**Expected pattern for `set -g status-bg red; set -go status-bg blue`:**
```
TRACE set-option: key="status-bg" only_if_unset=false unset=false -> write
TRACE user_set_options inserted: "status-bg" (set size: 1)
TRACE set-option: key="status-bg" only_if_unset=true unset=false -> noop (already in user_set_options)
```

**Verify:** 4 unit tests in src/config.rs (`set_option_o_*`) cover this —
run with `cargo test --bin psmux --lib config::tests::set_option_o`.

---

## P0.3 — Shared octal codec

**Invariant:** `decode_octal(encode_octal(x)) == x` for all ASCII input.
Codec location changed from `src/remote/octal.rs` to crate root `src/octal.rs`;
all imports must resolve.

**Trace points:** none needed — pure refactor, existing unit tests guard
correctness.  If remote protocol breaks with a resolution error, check
`src/remote/protocol.rs` for stale `super::octal` references.

---

## P0.5 — for_each_pane helper

**Invariant:** read-only DFS traversal over all panes in a Node, visits
each leaf once.  No unit test because Pane owns a real PTY.

**Trace points:** instrument the caller (control-mode server, when it
lands) with `log::trace!` at entry/exit of the traversal.

---

## P1.8 — capture-pane negative offset clamp

**Invariant:** `capture-pane -S -1` and `-E -1` clamp to row 0 of the
visible area (no scrollback is exposed here).

**Trace points:**
- `src/copy_mode.rs::capture_active_pane_range` and `capture_active_pane_styled`
  — log the resolved `start` and `end` rows

**Expected pattern for `capture-pane -S -3 -E -1 -p` on an 8-row pane:**
```
TRACE capture_active_pane_range: s=Some(-3) e=Some(-1) last_row=7 -> start=0 end=0
```

**Verify:**
```powershell
psmux capture-pane -S -5 -p | Measure-Object -Line   # must be 1 line (row 0)
```

---

## P1.9 — respawn-pane -k wired end-to-end

**Invariant:** `respawn-pane -k` on a living pane kills it and respawns;
without `-k`, returns an error with message "pane still active".

**Trace points:**
- `src/server/connection.rs` respawn-pane arm — log parsed kill flag
- `src/window_ops.rs::respawn_active_pane` — log (pane.dead, kill) at entry

**Expected pattern (with -k on a live pane):**
```
TRACE respawn-pane parsed args: kill=true
TRACE respawn_active_pane: dead=false kill=true -> killing process tree
TRACE respawn_active_pane: respawning shell in pane id=%1
```

**Verify:**
```powershell
$proc = psmux display-message -p '#{pane_pid}'
psmux respawn-pane -k -t %1
Start-Sleep -Milliseconds 300
$new = psmux display-message -p '#{pane_pid}'
$proc -ne $new   # must be True
```

---

## P1.10 — show-options -gv default-shell path resolution

**Invariant:** `show-options -gv default-shell` returns the resolved
shell path when `default_shell` is empty, not an empty string.

**Trace points:** `src/server/options.rs::get_option_value` match arm for
"default-shell" — log cached_shell() result.

**Verify:**
```powershell
psmux show-options -gv default-shell   # must not be empty — e.g. C:\Program Files\PowerShell\7\pwsh.exe
```

---

## P1.12 — MIN_SPLIT_ROWS respects pane-border-status

**Invariant:** splitting a 5-row parent vertically must **fail** on ohboy
(would leave 1 content row per pane after the title bar).  Splitting a
7-row parent must **succeed** (≥2 content rows per pane).

**Trace points:** `src/pane.rs` split_active_with_command — log the
min-rows check result.

**Verify:**
```powershell
# Attach to a manually sized small window, then:
psmux split-window -v   # must error if window height ≤ 5 rows after title bar
```

---

## P1.13 — defaults_suppressed reset on source-file reload

**Invariant:** after `source-file ~/.psmux.conf`, if the new config does
not contain `unbind-key -a`, `app.defaults_suppressed` becomes false.

**Trace points:** `src/server/mod.rs::CtrlReq::SourceFile` arm — log the
reset + post-parse value of defaults_suppressed.

**Verify:**
```powershell
# Config with unbind-key -a:
Set-Content ~/.psmux-test.conf 'unbind-key -a'
psmux source-file ~/.psmux-test.conf
psmux list-keys | Measure-Object -Line   # small count

# Remove unbind-key -a and reload:
Set-Content ~/.psmux-test.conf '# no unbind'
psmux source-file ~/.psmux-test.conf
psmux list-keys | Measure-Object -Line   # must be large — defaults restored
```

---

## P1.15 — status-format[0] override

**Invariant:** when `status-format[0]` is set (non-empty) and there is no
active display-message, line 0 of the status bar renders that template
via `parse_inline_styles` instead of the default layout.

**Trace points:** `src/client.rs` line 0 render — log the branch taken
(`use_status_format_0` vs default) and the rendered width.

**Verify:**
```powershell
psmux set -g status-format[0] '#[fg=red]RAILS_SYNC_ACTIVE'
# Visual: status bar shows 'RAILS_SYNC_ACTIVE' in red, no session/windows/tabs.
```

---

## P1.19 — split-window -t MRU pollution

**Invariant:** `split-window -t %2` while active pane is %1 keeps %1 at
MRU head, not %2.  `kill-pane` after split picks the previously-active
pane, not the split target.

**Trace points:**
- `src/server/connection.rs::is_focus_cmd` decision — log the result for
  `split-window`
- `src/server/mod.rs::FocusPaneTemp` — log focus_pane_by_id_no_mru call
- `src/pane.rs::kill_pane_at_path` — log mru_target vs fallback selection

**Verify:** 4 ps1 tests in `tests/test_pane_mru.ps1` — run on a real session.

---

## P2.25 — CREATE_NO_WINDOW for background subprocesses

**Invariant:** spawning a helper subprocess (run-shell, if-shell,
format#(), copy-pipe, toast) must not flash a conhost/cmd/pwsh window.

**Trace points:** `src/platform.rs::HideWindowCommandExt::hide_window` —
one-line trace confirming the flag set.

**Verify:**  `tests/test_hide_window_e2e.ps1` (to be ported from upstream) uses
Win32 EnumWindows to assert zero ConsoleWindowClass windows appear.
Manual check: spawn 10 `run-shell 'echo hi'` in a loop and watch for flashes.

---

## How to add a trace point

1. Pick the boundary your fix touches.
2. Add `log::trace!("short scope: key=value …", …);` — lower-case scope,
   key=value pairs, no adjectives.
3. Gate expensive state behind `log::log_enabled!(log::Level::Trace)` if
   the render cost is non-trivial.
4. Run with `RUST_LOG=psmux=trace` and `| Select-String <scope>` to filter.

When a trace point reveals the bug, **leave the trace in place** — it is
now documentation of the invariant.  Only remove a trace if the code
path is deleted.
