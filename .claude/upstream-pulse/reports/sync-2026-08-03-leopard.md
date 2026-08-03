# sync-2026-08-03-leopard

> ## ⚠️ POST-REVIEW CORRECTIONS (Fable, 2026-08-03 — all independently verified)
>
> **1. `dbfa42d` shipped a live credential-loss race. This report missed it entirely.**
> `server/mod.rs:3667-3671` (ClaimSession, the warm-pool claim path) removes the OLD `.port`,
> writes the NEW `.port`, and only THEN reads the OLD `.key`. Between the removal and the read,
> the old `.key` has no `.port` sibling — so pass 2 of `cleanup_stale_state_in` deletes it, the
> `if let Ok(key)` branch is skipped, and **the new `.key` is never written**. The session ends up
> with a valid `.port` and no credential: permanently unauthenticatable. The `.pipe` rename at
> :3680-3685 is guarded by `exists()` and loses backend discovery the same way. `RenameSession`
> (:3620-3624) has the identical shape. **This is a bug in pushed code and needs fixing now, not
> at merge time.**
>
> **2. This report's proposed remedy for #496 was wrong.** Moving pass 2 into `run_server` start
> only does NOT close the race: two servers starting concurrently (warm-pool replenish, orchestrate
> provisioning, CC team fan-out — all normal here) still let A's sweep hit B's pre-`.port` window.
> It also silently disables cleanup on machines that only ever run client-side commands.
> **Use the mtime age-gate instead** (skip sidecars younger than ~60s), which holds in every
> invocation context and also covers the mid-life rename races in correction 1.
>
> **3. The fork carries #496's root cause itself.** `server/mod.rs:698-705` truncates and rewrites
> `.key` AFTER `.port` is visible — the exact block upstream's `0b962b0` deletes. #496 is not only
> a threat to `dbfa42d`; it is a fix the fork needs.
>
> **4. Tier 1 item 2 (#492/#493) is OVERSTATED — the designs compose, they don't compete.**
> Upstream's `try_direct_spawn` returns `None` on shell metacharacters and on a leading `&`, so
> CC's teammate string (`cd X && env VAR=VAL claude …`) never takes the direct path and
> `6ba95ba`'s shim remains necessary and untouched. They genuinely conflict only for plain
> explicit-path commands. Two landmines: `b65b8fd` alone is a regression (its PATH-lookup spawn
> broke `timeout`-style sessions upstream) — `73ee0e2` is inseparable; and upstream's builder sets
> `PSMUX_SESSION="1"`, which would violate the fork's PSMUX_* env contract.
>
> **5. Largest miss: `7d6300b` "Fix BSOD kill guard plus fifteen bugs".** Upstream root-caused two
> kernel bugchecks (0xEF CRITICAL_PROCESS_DIED) to psmux terminating a session-0 `svchost` via a
> recycled-PID parent link. Same hazard family as #404/#510 — which this report ranks #1 — but
> strictly worse: it reboots the machine. Belongs in the safety wave. Its fix 15 (some Windows
> hosts never refuse unbound loopback ports) also bears on `port_file_is_live`
> (`session.rs:143-153`), which treats one 50ms timeout as dead — the aggressive failure mode.
>
> **6. Other misses:** #495 (`0ee01d6`/`aef2d1d`) and `24f1a5e` belong to the same `pane.rs` seam;
> #511 run-shell cluster; #478 (`0499c58`) client-attach accounting (touches FleetView's
> `state_version` neighbourhood); #479 mouse-protocol selection; #497 window-id target resolution.
> **`2268b74` (#474) is four separable defects, not a monolith** — defect 1 (MSYS2 unsets
> `USERPROFILE` → the startup reaper terminated every live server) is kill-scoping family and is
> acute on this machine, which runs Git Bash as `default-shell`.
>
> **7. `688db46` (#524 `send-keys -H`) → WONT DO.** The fork already has `CtrlReq::SendKeysHex`
> (`types.rs:1165`). This report listed it as a Tier 5 item to port.
>
> **8. "Extend `SERVER_STATE_EXTS` for `.sid`/`.pid`" is moot as written** — the fork has no
> `.sid`/`.pid` files, no `ensure_session_registry_files`, and no `paths.rs`.
>
> Merge strategy (waves 0-5, conflict resolution, do-not-port list, permanent divergences) is in
> the backlog's leopard section.

**Previous:** `sync-2026-07-12-tiger` (2026-07-12) · **Elapsed:** 22 days

## Sources

| Source | From | To | Delta |
|---|---|---|---|
| psmux/psmux | `f8013e7` | `941a862` | **128 commits** |
| Claude Code | 2.1.207 | **2.1.220** (installed) | 13 releases |
| Pi coding agent | 0.73.1 | 0.73.1 | **no change** |

Total divergence `HEAD..upstream/master` is 601 commits; 128 of those are new since tiger.
Upstream shipped **Release 3.3.7** (`05cc5d4`) in this window.

Change concentration: `src/` 141 files, `src/server/` 48, `tests-rs/` 83, `tests/` 185.

---

## TIER 1 — MERGES (3)

### 1. `0b962b0` (#496) — credential writes moved BEFORE the `.port` beacon ⚠️ **CONFLICTS WITH `dbfa42d`**

Upstream **inverts** the state-file write order: `.key`/`.sid` are now written *before*
`.port`, because `.port` is the readiness beacon clients poll. Their commit comment is
explicit: *"Writing .port first opened a window where a cold-start attach read an empty
.key and failed AUTH."*

**This directly contradicts the invariant today's `dbfa42d` relies on.** Our new
`session.rs::cleanup_stale_state_in` pass 2 removes sidecars that have no `.port` sibling,
and is documented as safe *because* `run_server` writes `.port` first. Port #496 as-is and
`cleanup_stale_port_files()` — which runs on **every** psmux invocation (`main.rs:85`) —
gains a window in which it can delete a *starting* server's `.key`, reintroducing exactly
the cold-start AUTH failure #496 fixed.

**Action:** port #496 and `dbfa42d` together, not independently. Either (a) gate pass 2 on
file age (skip sidecars younger than ~60s), (b) key pass 2 on a file written last, or
(c) move pass 2 out of the per-invocation path into `run_server` start only. Option (c) is
cheapest and matches `prune_crashes`/`prune_snapshots_in`. Also extend `SERVER_STATE_EXTS`
— upstream now has `.sid` and `.pid` files that ohboy does not.

### 2. `b65b8fd` + `73ee0e2` (#492, #493) — spawn plain pane commands directly, no shell wrapper

`src/pane.rs` +126. Upstream's answer to the same problem today's `6ba95ba` solved with a
POSIX `env` shim: don't wrap the command in a shell at all. `73ee0e2` then limits direct
spawn to explicit executable paths.

**Two competing designs for one seam.** ohboy's `6ba95ba` honours `--shell`/`default-shell`
and installs an `env` shim for pwsh; upstream bypasses the shell entirely. ohboy also has
`new-window --raw` already covering part of this.

**Action:** decide the seam before porting anything else in `pane.rs`. Reconcile against
`build_command` and the 7 `pane::test_posix_env_shim_command_panes::*` tests. Do **not**
cherry-pick — concept-port per the Porting Guardrails.

### 3. `2268b74` (#474) — Git Bash, MSYS2, zsh as first-class `default-shell` backends

Touches `client.rs`, `main.rs`, `pane.rs`, `paths.rs`, `platform.rs`, `session.rs`,
`ssh_input.rs` (+683). Overlaps `6ba95ba` Fix B (honour `default-shell` on command panes).
This machine already runs Git Bash as `default-shell` (`~/.psmux.conf:3`), so behaviour
here is directly observable.

**Action:** port after item 2 is decided — both rewrite the same shell-resolution path.
Depends on `bb2df62` (paths.rs centralisation).

---

## TIER 2 — QUICK CONFIG (2)

- **`c938280`** — make every option-catalog default match the runtime. Small, mechanical,
  removes a class of `show-options` lies. Verify against ohboy's diverged `options.rs`.
- **`6ff30c1`** (#489) — tmux-style `-e KEY=VALUE` on new-window/split-window.
  **Likely WONT DO:** ohboy shipped this as `5cf217b` (2026-03-20). Verify parity, then
  mark WONT DO rather than porting.

---

## TIER 3 — LOW COMPLEXITY / HIGH IMPACT (2)

### `3cf9a07` + `2b72065` (#404) — scope the kill-server force-kill fallback to the data dir, by PID identity

`session.rs` -135/+177, plus a 126-line test suite. **This is the fix for the single worst
hazard in this repo**: `kill_remaining_server_processes()` currently does a by-name
`TerminateProcess` sweep over every `psmux.exe`/`pmux.exe`/`tmux.exe` on the machine. That
is why a bare `cargo test` destroys every live session (see memory
`cargo-test-nukes-all-live-psmux-sessions`) and it is why four orphan servers had to be
killed by hand today.

**Action:** port early. Highest safety-per-line in this window.

### `d7d5b50` + `c257249` + `06753e4` (#510) — reap only servers this data dir can positively claim

Same family: stops a foreign data dir reaping our servers. Pairs naturally with #404.

---

## TIER 4 — FIXES (~30)

**IPC / one-shot command reliability** — the tiger queue's #1 item, now with more on top:
- `886ca2e` + `1b80e90` (#466) — confirm command **execution** via a session-info FIFO
  barrier (`connection.rs` +6, `session.rs` +13; small and high value)
- `c41a8ba` (#499) — quoted semicolons no longer split the one-shot command line
- `d981d94` — `#()` format expands synchronously for one-shot callers

**Server identity / name guard** (supersedes tiger's `3ce9d65`):
- `2f4763d` — bare invocation silently replacing a live server for its own session name
- `b3d55f8` (#505) — re-key the single-server name guard on session rename
- `b96d01b` + `8b0ba69` (#509) — stable server identity per `-L` namespace

**Claude Code teammate path:**
- `67780ca` (#475) — teammate wrapper resolves the `claude` command instead of hardcoding
  `claude.exe`. Relevant to the teammate-launch story.
- `fd5e585` (#399) — respect configured `teammateMode` over auto-injection

**Terminal correctness:** `c77fd02` (#502, per-screen DECSC so nvim colours stop leaking),
`f154008`/`b6ca300` (#504/#508, NUL folded onto C-Space), `3d5ec4e`/`a5e96dd` (OSC 52
parity), `d624794` (#443, blank-cell backfill on every capture/copy path), `dbe7222`
(cross-session join-pane), `9209733` (#494, freeze copy-mode screen during output),
`caa12db` (#491, stop Ctrl+C killing wsl.exe), `d134533` (gate mouse clicks on explicit
mouse mode), `061ac56` (#485, display-message wrong session inside a pane), `b6c96fd` +
`6231fa0` (#490, send-keys whitespace/token fidelity), `775866a` (#472) / `a679a2e` (#310)
(prefix table binding), `93240f5` (#471) / `376f7e1` (#470) / `30f15fe` (#507) (popup),
`d56d777` (#482/#483) + `6c76ff9` (pipe-pane flags, switch-client targets), `eb5bee1`
(#476, bind-key quoting), `06247bf` (#450, shells born with corrupt std handles),
`cf72162` (#526), `7bf11f3` (#525), `534ddb9` (#481, caret/mouse row mapping).

---

## TIER 5 — FEATURES (~15)

`2d4203d`+`f254ee7`+`170391b` async `#(command)` status expansion (non-blocking format
jobs) · `843105b`+`290343a` (#498) copy-mode set-mark/jump-to-mark/`{ }`/`z` ·
`688db46` (#524) `send-keys -H` literal byte injection · `44de2c8` (#469)
`pane_current_command` derived from shell-integration OSC · `922b464` (#450) opt-in
`@heal-crashed-panes` · `ea71e75` session picker filtering · `0d90e22` `tests/monitor`
live TUI dashboard · `b96f6de` (#473) OSC 4/10/11 + CSI ?996n colour queries ·
`21f268b`+`dcd8bf1`+`a7ab6fb` raw VT clients over SSH without ConPTY · `913f5e9` (#488)
PageUp forwarding · `4921068` (#501) cache the process-table walk on the format render path
· `bb2df62` centralise data-dir paths in `paths.rs` (**enabler for Tier 1 item 3**) ·
`1a8b6d5`+`615d2a5`+`2474a54` dependency bumps clearing two RUSTSEC unsoundness advisories
· `ee97ce9` CI gating on security advisories · `720a581` embed exact git commit in version
(**ohboy already has this** — #110 Feature 2, WONT DO).

---

## Claude Code 2.1.207 → 2.1.220

**No tmux/pane backend protocol changes in this window** — the psmux integration surface is
stable. Nothing touches `CLAUDE_PANE_BACKEND_SOCKET` or the teammate launch sequence that
`576b4fe`/`6ba95ba` were built against.

Relevant anyway:
- **2.1.219** — Claude Opus 5 default; **subagent nesting up to depth 3**. Deeper nesting
  multiplies pane spawns; ohboy's practical cap is 5–6 panes (min-pane-size). Worth a
  bound before running deep teams.
- **2.1.216** — fixed resumed sessions with agent teams losing lineage after compaction;
  agent frontmatter hooks needing workspace trust.
- **2.1.212** — `/subtask` replaces subagent; WebSearch/subagent spawn limits; `/fork`
  creates a background session.
- **2.1.214** — `EndConversation` tool. **2.1.211** — `--forward-subagent-text`.

## Pi coding agent

0.73.1 → 0.73.1. **No change.** `pi-dispatch.ps1` / `pi-swarm.ps1` unaffected.

---

## REGRESSION RISK

```
⚠ State-file cleanup (dbfa42d, TODAY) — #496 inverts the .port-before-sidecar
  ordering that cleanup_stale_state_in pass 2 is documented as depending on.
  Verify: cargo test --bin psmux test_stale_state_cleanup, plus a cold-start
  attach race, BEFORE and AFTER porting #496.
⚠ POSIX env shim / default-shell (6ba95ba) — #492/#493 and #474 both rewrite the
  same shell-resolution seam in pane.rs.
  Verify: cargo test --bin psmux posix_env_shim (7 tests).
⚠ kill-server / warm pool — #404 and #510 rewrite session.rs reaping wholesale;
  ohboy has kill_warm_servers() and reserve_pane_id_base() in that file.
  Verify: pwsh tests/run_rails_bench.ps1 (pane_id_unique especially).
✓ CustomPaneBackend / DCS passthrough / FleetView — no upstream commits touch
  src/backend/, crates/vt100-psmux DCS handlers, or crates/psmux-fleet.
```

## Recommended order

1. `3cf9a07`+`2b72065` (#404) and `d7d5b50` (#510) — kill-server scoping. Safety first,
   and it retires the `cargo test` hazard.
2. `886ca2e` (#466) + `c41a8ba` (#499) — one-shot command execution barrier. Small, and it
   is the flakiness that bites `orchestrate`/`exec`/`wait-for`.
3. Decide the `pane.rs` shell seam (Tier 1 items 2+3) before any further `pane.rs` port.
4. `0b962b0` (#496) **paired with** a `dbfa42d` pass-2 adjustment.
5. Everything else.
