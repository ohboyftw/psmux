# On-Rails Checklist

Priority-ranked task list for keeping ohboy-builds and upstream psmux in sync. Tasks are scored by **sync value** (divergence reduction / future-merge cost) not feature value.

Status legend: `[ ]` todo · `[x]` done · `[~]` in progress · `[-]` skipped with reason

---

## P0 — Structural Rails (unblockers)

Without these, every future upstream merge hurts more. Do these first.

- [x] **1. Port `allow_set_title` + OSC 0/2 title propagation** — commits `cdacbf4`, `1d416b6`
- [x] **2. Fix `set-option -o` via `user_set_options` HashSet** — commit `da27eac`
- [x] **3. Extract shared `src/octal.rs` codec** — commit `59bcba6`
- [-] **4. Port 64KB output ring buffer to `src/pane.rs`** — DEFERRED: dead code without control-mode consumer; bundle with control-mode server port
- [x] **5. Port `for_each_pane()` helper to `src/tree.rs`** — commit `e4cb3f4`

---

## P1 — High-Impact Bug Ports on Shared Surface

Upstream fixes where ohboy currently has the buggy version. Silent behavioral drift.

- [-] **6. Fire `pane-died`/`pane-exited` hooks when remain-on-exit enabled** — N/A: ohboy emits `context_exited` push events + mycel topics instead of upstream's hook names
- [-] **7. Bounded frame channel preserves fast typing** (`0d16017`) — DEFERRED: ohboy's `register_frame_slot()` signature diverged (no `client_id` parameter); bigger refactor than upstream diff, pair with next control-mode pass
- [x] **8. `capture-pane` negative `-S`/`-E` clamps to row 0** — commit `ec22ed0`
- [x] **9. `respawn-pane -k` silently discarded at server** — commit `57996e3`
- [x] **10. `show-options -gv`/`-wv` without name lists all values** — commit `2ba6b4b`
- [-] **11. 8 tmux command flag compatibility gaps** (#209, `6118034`) — DEFERRED: needs per-flag decomposition; land as a separate sync batch
- [x] **12. Lower `MIN_SPLIT_ROWS` 4→2** — commit `57996e3` (ohboy uses **3**, not upstream's 2, because `pane-border-status="top"` steals 1 row per pane — see Incident log in `ohboy-builds-backlog.md`)
- [x] **13. `defaults_suppressed` reset on source-file reload** — commit `2ba6b4b`
- [x] **14. Bell/silence flags clearing in check_window_activity** — already in ohboy (`src/server/helpers.rs::check_window_activity` already clears all three flags on active window; cites #162)
- [x] **15. status-format[] inline styles use parse_inline_styles** — commit `2ba6b4b` (part 2: status-format[0] override); part 1 (lines 1+) already in ohboy
- [x] **16. status-right/left empty clearing** — already in ohboy (`src/client.rs:2966` cites #160)
- [-] **17. send-keys C-x/M-x local path parsing** (#230, `d47b65c`) — N/A: the local fallback code path that had the bug doesn't exist in ohboy; all send-keys routes through the server
- [x] **18. resize direction fix on bottom/right edges** — already in ohboy (`src/window_ops.rs::resize_pane_vertical` and `resize_pane_horizontal` use mathematically-equivalent but cleaner code; cites #81)
- [x] **19. split-window MRU pollution** — commit `4eda6c3`

---

## P2 — Verification + Agent-Adjacent

- [x] **20. Verify session.rs namespace functions exist** — INTENTIONALLY ABSENT: ohboy uses CustomPaneBackend pipe discovery for session scoping, not upstream's `-L` socket namespace helpers. Documented in `tests/verify_parity.ps1`
- [x] **21. Verify nesting prevention guards** — PASS (verify_parity.ps1)
- [x] **22. Verify VTI/warmup/kill-server fixes** — PASS (verify_parity.ps1)
- [x] **23. `new-session -e` env var support** — already in ohboy (`src/main.rs:1197`, `src/server/connection.rs:385`)
- [x] **24. PredictionViewStyle fix** — already in ohboy (commit `1d8757b`)
- [x] **25. CREATE_NO_WINDOW for background subprocesses** — commit `2469d49` (HideWindowCommandExt trait + 6 spawn sites)

---

## P4 — Explicitly Parked (not rails work)

Document the divergence; don't port.

- Cross-session join-pane via TCP proxy — ohboy has CustomPaneBackend for same use case
- Remove legacy `src/app.rs` — ohboy uses app.rs for own state; would be multi-day refactor
- Options catalog `option_catalog.rs` — ohboy uses `src/server/options.rs` instead
- pwsh-mouse-selection (#211) — interactive polish, no agent value
- Tier 4 interactive-only (window-name targeting, if-shell -F, run-shell async, bind-key command mode, resize/split/layout)
- Layout directives (`e584cbe`) — large style.rs rewrite
- Shift+Enter 4 bugs (`cfb71bc`) — high-risk input refactor; ohboy input diverged on purpose

---

## Execution Notes

- **BEFORE every port**, run the Porting Guardrails checklist in `docs/ohboy-builds-backlog.md` — verify the upstream invariant still holds against ohboy's N+k extensions (title bars, CustomPaneBackend, passthrough, warm pane, etc.)
- After each P0/P1 item: `cargo fmt && cargo clippy -- -D warnings && cargo test --lib`
- Commit each logical chunk separately with `fix(sync):` or `refactor(sync):` prefix
- Update Completed table in `docs/ohboy-builds-backlog.md` after each landed task
- When a port breaks an ohboy-only assumption: add a row to the **Incident log** table so future ports don't repeat the miss
- When ohboy adds a new cardinal extension (new global option, new Pane field, new render-layer feature): add it to the **Known Ohboy-Only Extensions** table
- Bump `.claude/upstream-pulse/state.json` when finishing a batch
