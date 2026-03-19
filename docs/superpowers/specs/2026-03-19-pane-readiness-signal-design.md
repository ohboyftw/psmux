# Pane Readiness Signal + Swarm Test Fixes

**Date:** 2026-03-19
**Status:** Draft
**Scope:** Test infrastructure fix + psmux feature addition

## Problem

When Claude Code spawns agents into psmux panes via `send-keys`, the pane's PowerShell shell may still be loading (PSReadLine, profile, etc.). Commands arrive before the prompt is ready and get swallowed. This causes:

- E2E test failures (6 of 48 tests fail due to shell not ready)
- Potential reliability issues in real swarm workflows
- No programmatic way to check if a pane's shell is accepting input

## Design

### Part 1: Test Fix — `Wait-PaneReady` Helper

Add a function to `test_swarm_e2e.ps1` that confirms a pane's shell is ready before sending real commands.

```powershell
function Wait-PaneReady {
    param([string]$Target, [int]$TimeoutSeconds = 20)
    $sentinel = "RDY_$(Get-Random)"
    # Keep sending the sentinel until it appears in capture-pane output
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        & $PSMUX send-keys -t $Target "echo '${sentinel}'" Enter
        Start-Sleep -Milliseconds 800
        $cap = Get-PaneOutput $Target
        if ($cap -match $sentinel) { return $true }
    }
    return $false
}
```

Apply to all tests that inspect env vars or write files from panes.

### Part 2: psmux Feature — Pane Readiness Signal

#### 2a. `last_output_time` field (types.rs)

Add to `PaneState`:
```rust
pub last_output_time: Arc<AtomicU64>,  // epoch millis of last PTY output
```

Update PTY reader to record timestamp when `data_version` increments.

#### 2b. `#{pane_ready}` format variable (format.rs)

Returns `"1"` when:
- `data_version > 0` (pane has produced output)
- `now - last_output_time > 500ms` (output has stabilized)

Returns `"0"` otherwise.

#### 2c. `wait-pane --ready` flag (connection.rs)

Extend existing `wait-pane` handler:
```
psmux wait-pane -t %N --ready --timeout 10
```

Polls the pane's ready state every 100ms. Returns 0 when ready, 1 on timeout.

## Files Changed

| File | Change |
|------|--------|
| `tests/test_swarm_e2e.ps1` | Add `Wait-PaneReady`, apply to Phase 2/6 tests, fix 5.10 threshold |
| `src/types.rs` | Add `last_output_time: Arc<AtomicU64>` to PaneState |
| `src/server/mod.rs` | Update PTY reader to set `last_output_time` |
| `src/format.rs` | Add `pane_ready` format variable |
| `src/server/connection.rs` | Add `--ready` flag to `wait-pane` handler |

## Success Criteria

- All 48 E2E tests pass (or skip with documented reason)
- `psmux wait-pane -t %0 --ready --timeout 10` blocks until shell prompt appears
- `psmux display-message -p "#{pane_ready}"` returns 1 for live panes
- `cargo clippy -- -D warnings` passes
- `cargo test` passes
