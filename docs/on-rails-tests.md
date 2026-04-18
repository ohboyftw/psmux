# On-Rails Test Plan

Integration + contract test strategy for the 25 P0/P1/P2 rails fixes on `sync/rails-p0-p2`.

## Test Harness Matrix

psmux has 4 testing layers. Each rails item uses the layer that matches its boundary.

| Layer | Location | Used for |
|-------|----------|----------|
| **Rust unit** | `#[cfg(test)]` modules in `src/` | Pure functions, data structure invariants (ring buffer, HashSet tracking, octal codec) |
| **Rust integration** | `tests-rs/` | Multi-module contract tests; serde round-trips; VT100 ↔ screen state diffs |
| **PowerShell E2E** | `tests/test_*.ps1` | Real server spawn, send-keys/capture-pane assertions, config file parsing, agent spawn path |
| **Contract (JSON-RPC)** | `tests/backend_contracts.rs` + new ps1 tests | CustomPaneBackend protocol: request/response shape, error schemas, field name stability |

## Boundaries to Validate

1. **JSON-RPC CustomPaneBackend** (`src/backend/protocol.rs`) — highest agent-regression risk
2. **tmux client ↔ server over named pipe** — binary message contract
3. **Config parser (`set -g foo bar`) → AppState** — text → types contract
4. **VT100 parser ↔ screen** — OSC/DCS/SGR events to state transitions
5. **Pipe discovery file (`~/.psmux/{session}.pipe`)** — filesystem contract for Pi / agent tooling

## Per-Task Test Specs

### P0.1 — `allow_set_title` + OSC 0/2 title propagation

- **Contract test** (Rust): OSC 0/2 bytes fed to VT parser → `screen.pane_title` mutates; test in `crates/vt100-psmux/tests/`
- **Integration test** (Rust, new `tests-rs/test_osc_title_propagation.rs`):
  - Spawn pane, write `\x1b]2;my-title\x07`, assert `format!("#{{pane_title}}")` returns `"my-title"`
  - `set -g allow-set-title off` → OSC ignored
  - `select-pane -T fixed` then OSC → `title_locked=true` blocks OSC update
- **E2E** (`tests/test_osc_title_e2e.ps1`): real shell emits OSC, capture via `psmux display-message -p '#{pane_title}'`

### P0.2 — `set-option -o` via `user_set_options` HashSet

- **Unit test** (inside `src/config.rs`):
  - Fresh AppState: `set-option -o foo bar` → inserts, value=`bar`
  - Second `set-option -o foo baz` → no change (already in set)
  - `set-option -u foo` → removes from set
  - Reload via source-file does NOT clear the set
- **Regression test** (new `tests-rs/test_set_option_only_if_unset.rs`): covers options with non-empty defaults (e.g., `status-bg`) — the exact upstream bug

### P0.3 — Extract shared `src/octal.rs` codec

- **Round-trip property test** (existing in `src/remote/octal.rs` tests — move with code):
  - `decode(encode(bytes)) == bytes` for arbitrary `Vec<u8>`
  - Edge cases: empty, all `\x00`, all `\xff`, embedded backslashes
- **No new tests needed** — pure refactor locked in by existing suite

### P0.4 — 64KB output ring buffer on `Pane`

- **Unit test** (`src/pane.rs` `#[cfg(test)]`):
  - Push < 64KB → retained intact
  - Push exactly 64KB → retained intact, capacity constant
  - Push 65KB → oldest 1KB evicted, newest 64KB retained
  - Concurrent reader+writer via Arc/Mutex → no data loss under serialized access
- **Invariant**: memory footprint never exceeds 64KB per pane

### P0.5 — `for_each_pane()` helper on `src/tree.rs`

- **No direct unit test** — `Pane` owns `Box<dyn MasterPty>` / `Child` so construction requires a real PTY spawn. The sibling `visit_leaves_mut` has no unit test for the same reason.
- **Coverage via caller tests**: when control-mode or any other feature starts using `for_each_pane`, that feature's integration tests exercise the traversal.
- **Inspection-only guarantee**: the body is a 5-line mechanical mirror of `visit_leaves_mut` — correctness by construction.

### P1 batch — 14 upstream bug ports

Pattern per fix: **1 unit test of the fix** + **1 E2E test of the user-visible behavior**.

| Item | Unit test | E2E test |
|------|-----------|----------|
| 6. pane-died hooks when remain-on-exit | hook dispatch invoked path | `tests/test_remain_on_exit_hooks.ps1` |
| 7. bounded frame channel | channel capacity + ordering | keystroke-rate stress test |
| 8. capture-pane neg -S clamp | `-S -5` returns row 0 | `tests/test_capture_pane_neg_offset.ps1` |
| 9. respawn-pane -k | `-k` flag propagates | `tests/test_respawn_pane_k.ps1` |
| 10. show-options -gv no name | returns full map | `tests/test_show_options_global.ps1` |
| 11. #209 8-flag parity | per-flag assertions | `tests/test_issue209_flag_parity.ps1` |
| 12. MIN_SPLIT_ROWS 4→2 | const value | `tests/test_split_small_pane.ps1` |
| 13. defaults_suppressed reset | source-file reload clears flag | `tests/test_source_file_unbind_all.ps1` |
| 14. bell/silence flags clearing | all three flags cleared | `tests/test_window_activity_flags.ps1` |
| 15. status-format inline styles | parse_inline_styles path used | `tests/test_status_format_lines.ps1` |
| 16. status-right/left empty | empty string clears line | `tests/test_status_clear.ps1` |
| 17. send-keys C-x/M-x | path parser matches server | `tests/test_send_keys_modifiers.ps1` |
| 18. resize direction | edge-case panes resize correctly | existing `test_issue81_resize_direction.rs` |
| 19. split-window MRU | focus target unchanged | `tests/test_pane_mru.ps1` |

### P2 verify items (20–22)

- **Script** (`tests/verify_parity.ps1`): greps for namespace fns, nesting guards, VTI/warmup paths. Reports pass/fail per item. Run in CI.

### P2 agent-adjacent (23–25) — **REGRESSION HOTSPOT**

Highest-stakes tests since they directly touch CustomPaneBackend.

- **#23 new-session -e** — `tests/test_new_session_env.ps1`:
  - `psmux new-session -e FOO=bar -d` → spawned pane has `$env:FOO -eq 'bar'`
  - Multiple `-e` flags stack
  - Contract: JSON-RPC `exec` accepts `env` map (if added)

- **#24 PredictionViewStyle / warm pane respawn** — `tests/test_warm_pane_config_reload.ps1`:
  - Config with `allow-predictions on`
  - Server starts, warm pane spawned with default init string
  - After `load_config()` reads allow-predictions, warm pane re-spawned with correct PSReadLine init
  - Verify via send-keys + capture-pane that prediction view is active

- **#25 CREATE_NO_WINDOW** — `tests-rs/test_create_no_window.rs`:
  - All `CommandBuilder::new()` sites set `creation_flags` bit `CREATE_NO_WINDOW`
  - Integration: `tests/test_no_pwsh_flash.ps1` — spawn pane, poll window list for 500ms, assert no conhost/pwsh window appears standalone

**Regression gate for all three:** re-run `tests/validate-swarm-backend.ps1` (existing) after each. That script spawns 6 concurrent panes via CustomPaneBackend and asserts env vars, pipe discovery, send-keys reliability.

## Contract Snapshot Strategy (JSON-RPC)

- Capture current `list` / `exec` / `wait_for` responses to `tests/snapshots/backend/*.json` as golden files
- After each rails fix, replay snapshot inputs; assert response schema unchanged (ignore volatile PID/timestamp fields)
- Any new field is additive-only — breaking changes fail the gate

## Trace-Debug Integration

For each P0/P1 item that ships, run a `trace-debug` scenario:

1. Define expected behavior (1-line assertion)
2. Add `log::trace!` at each boundary the fix touches
3. Run with `RUST_LOG=psmux=trace`
4. Diff captured trace against expected
5. Attach trace excerpts to the PR/commit

## CI Gate

After each rails commit:

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --lib
cargo test --test '*'
pwsh tests/validate-swarm-backend.ps1
pwsh tests/verify_parity.ps1
```

Any red → block merge into master.
