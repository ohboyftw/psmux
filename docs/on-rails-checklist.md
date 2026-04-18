# On-Rails Checklist

Priority-ranked task list for keeping ohboy-builds and upstream psmux in sync. Tasks are scored by **sync value** (divergence reduction / future-merge cost) not feature value.

Status legend: `[ ]` todo · `[x]` done · `[~]` in progress · `[-]` skipped with reason

---

## P0 — Structural Rails (unblockers)

Without these, every future upstream merge hurts more. Do these first.

- [ ] **1. Port `allow_set_title` + OSC 0/2 title propagation** (Tier 3)
  - Adds `allow_set_title: bool` to AppState, `title_locked: bool` to Pane, `propagate_osc_titles()` helpers, `set -g allow-set-title` config parsing
  - Files: `src/types.rs`, `src/server/helpers.rs`, `src/server/mod.rs`, `src/config.rs`, `src/server/options.rs`, `docs/pane-titles.md`
  - Unblocks: hostname pane_title default, pane title in border format, rsel cleanup, VT100 OSC capture
  - Risk: Medium (types.rs + server/mod.rs diverged)

- [ ] **2. Fix `set-option -o` via `user_set_options` HashSet** (Tier 5, `b6098bb`)
  - Replace `is_empty()` check in `src/config.rs:476` with HashSet membership
  - Files: `src/config.rs`, `src/types.rs` (add field), every set_option call site
  - Risk: Low — localized

- [ ] **3. Extract shared `src/octal.rs` codec** (Control Mode Tier 5)
  - Move `src/remote/octal.rs` → `src/octal.rs`, update imports
  - Risk: Zero (pure refactor)

- [ ] **4. Port 64KB output ring buffer to `src/pane.rs`** (Control Mode Tier 5)
  - Additive ~38 lines, useful for control mode + capture-pane
  - Risk: Low (additive)

- [ ] **5. Port `for_each_pane()` helper to `src/tree.rs`** (Control Mode Tier 5)
  - Additive ~10 lines
  - Risk: Zero (additive helper)

---

## P1 — High-Impact Bug Ports on Shared Surface

Upstream fixes where ohboy currently has the buggy version. Silent behavioral drift.

- [ ] **6. Fire `pane-died`/`pane-exited` hooks when remain-on-exit enabled** (`2a12809`+`80bb22e`)
- [ ] **7. Bounded frame channel preserves fast typing** (`0d16017`)
- [ ] **8. `capture-pane` negative `-S`/`-E` clamps to row 0** (`4bf69e0`)
- [ ] **9. `respawn-pane -k` silently discarded at server** (`88daff3`)
- [ ] **10. `show-options -gv`/`-wv` without name lists all values** (`bfa87f6`)
- [ ] **11. 8 tmux command flag compatibility gaps** (#209, `6118034`) — decompose before porting
- [ ] **12. Lower `MIN_SPLIT_ROWS` 4→2 in `src/pane.rs`** (`b6098bb`)
- [ ] **13. `defaults_suppressed` reset on source-file reload** (`266d414`)
- [ ] **14. Bell/silence flags clearing in check_window_activity** (`a2f853c`)
- [ ] **15. status-format[] inline styles use parse_inline_styles** (`b9be26a`)
- [ ] **16. status-right/left empty clearing** (`ae47fee`)
- [ ] **17. send-keys C-x/M-x local path parsing** (#230, `d47b65c`)
- [ ] **18. resize direction fix on bottom/right edges** (`a8e3852`)
- [ ] **19. split-window MRU pollution** (`d8c4d01`)

---

## P2 — Verification + Agent-Adjacent

- [ ] **20. Verify session.rs namespace functions exist** (`3e61a0d`) — check only
- [ ] **21. Verify nesting prevention guards** (`3f95642`) — check only
- [ ] **22. Verify VTI/warmup/kill-server fixes** (`e29b954`) — check only
- [ ] **23. `new-session -e` env var support** (#205) — aligns ohboy's agent spawn env with standard mechanism
- [ ] **24. PredictionViewStyle fix** — warm pane is agent fast-path; direct CustomPaneBackend impact
- [ ] **25. CREATE_NO_WINDOW for background subprocesses** — eliminates pwsh/cmd flash during agent spawn

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
