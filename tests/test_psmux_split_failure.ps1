# test_psmux_split_failure.ps1
# Reproduces the canopy spawn failure: psmux split-window silently fails
# when pane width is too small, returning exit 0 with empty pane ID.
#
# Expected behavior: psmux should either:
#   (a) Return non-zero exit code when it can't split, OR
#   (b) Always return a valid pane ID (creating a new window if needed)
#
# Actual behavior: Returns exit 0 with empty stdout. No pane created.
# This causes canopy's spawner to call `send-keys -t ""` which fails.

$ErrorActionPreference = "Stop"
$session = "psmux-split-test-$(Get-Random)"
$failures = @()
$passed = 0
$failed = 0

Write-Host "=== psmux split-window failure reproduction ===" -ForegroundColor Cyan
Write-Host ""

# --- Test 1: Repeated horizontal splits exhaust width ---
Write-Host "[Test 1] Repeated horizontal splits until failure" -ForegroundColor Yellow

psmux new-session -d -s $session -x 120 -y 30
if ($LASTEXITCODE -ne 0) {
    Write-Host "  SKIP: Could not create session" -ForegroundColor Red
    exit 1
}

$maxSplits = 10
$emptyResults = @()

for ($i = 1; $i -le $maxSplits; $i++) {
    $result = psmux split-window -h -t $session -P -F "#{pane_id}" 2>&1
    $exitCode = $LASTEXITCODE

    if ($exitCode -ne 0) {
        # This is actually CORRECT behavior - non-zero exit on failure
        Write-Host "  Split $i : exit=$exitCode (error returned - GOOD)" -ForegroundColor Green
        break
    }

    if ([string]::IsNullOrWhiteSpace($result)) {
        # BUG: exit 0 but no pane ID returned
        $emptyResults += $i
        Write-Host "  Split $i : exit=0, pane_id='' (SILENT FAILURE)" -ForegroundColor Red
    } else {
        Write-Host "  Split $i : exit=0, pane_id='$result' (ok)" -ForegroundColor Green
    }
}

$panes = psmux list-panes -t $session 2>&1
$paneCount = ($panes | Measure-Object -Line).Lines
Write-Host ""
Write-Host "  Panes created: $paneCount" -ForegroundColor Cyan
Write-Host "  Splits attempted: $maxSplits" -ForegroundColor Cyan
Write-Host "  Silent failures (exit=0, no pane_id): $($emptyResults.Count)" -ForegroundColor $(if ($emptyResults.Count -gt 0) { "Red" } else { "Green" })

if ($emptyResults.Count -gt 0) {
    $failed++
    $failures += "Test 1: split-window returns exit 0 with empty pane_id on splits: $($emptyResults -join ', ')"
} else {
    $passed++
}

psmux kill-session -t $session 2>$null

Write-Host ""

# --- Test 2: Verify send-keys fails with empty pane ID ---
Write-Host "[Test 2] send-keys with empty pane target" -ForegroundColor Yellow

psmux new-session -d -s $session -x 120 -y 30
$result = psmux send-keys -t "" "echo hello" "Enter" 2>&1
$exitCode = $LASTEXITCODE

if ($exitCode -ne 0) {
    Write-Host "  send-keys -t '': exit=$exitCode (correctly errors)" -ForegroundColor Green
    $passed++
} else {
    Write-Host "  send-keys -t '': exit=0 (should have failed!)" -ForegroundColor Red
    $failed++
    $failures += "Test 2: send-keys with empty target returned exit 0"
}

psmux kill-session -t $session 2>$null

Write-Host ""

# --- Test 3: Minimum pane width boundary ---
Write-Host "[Test 3] Split at minimum width boundary" -ForegroundColor Yellow

# Create a narrow session (20 cols) - should fail on first split
psmux new-session -d -s $session -x 20 -y 30

$result = psmux split-window -h -t $session -P -F "#{pane_id}" 2>&1
$exitCode = $LASTEXITCODE

if ($exitCode -ne 0) {
    Write-Host "  20-col split: exit=$exitCode (correctly rejected)" -ForegroundColor Green
    $passed++
} elseif ([string]::IsNullOrWhiteSpace($result)) {
    Write-Host "  20-col split: exit=0, pane_id='' (SILENT FAILURE)" -ForegroundColor Red
    $failed++
    $failures += "Test 3: split-window in 20-col session returns exit 0 with no pane_id"
} else {
    Write-Host "  20-col split: exit=0, pane_id='$result' (split succeeded at 20 cols)" -ForegroundColor Green
    $passed++
}

psmux kill-session -t $session 2>$null

Write-Host ""

# --- Test 4: Vertical split exhaustion ---
Write-Host "[Test 4] Repeated vertical splits until failure" -ForegroundColor Yellow

psmux new-session -d -s $session -x 120 -y 30

$emptyResults = @()
for ($i = 1; $i -le $maxSplits; $i++) {
    $result = psmux split-window -v -t $session -P -F "#{pane_id}" 2>&1
    $exitCode = $LASTEXITCODE

    if ($exitCode -ne 0) {
        Write-Host "  VSplit $i : exit=$exitCode (error returned - GOOD)" -ForegroundColor Green
        break
    }

    if ([string]::IsNullOrWhiteSpace($result)) {
        $emptyResults += $i
        Write-Host "  VSplit $i : exit=0, pane_id='' (SILENT FAILURE)" -ForegroundColor Red
    } else {
        Write-Host "  VSplit $i : exit=0, pane_id='$result' (ok)" -ForegroundColor Green
    }
}

if ($emptyResults.Count -gt 0) {
    $failed++
    $failures += "Test 4: vertical split-window returns exit 0 with empty pane_id on splits: $($emptyResults -join ', ')"
} else {
    $passed++
}

psmux kill-session -t $session 2>$null

Write-Host ""

# --- Summary ---
Write-Host "=== Summary ===" -ForegroundColor Cyan
Write-Host "  Passed: $passed" -ForegroundColor Green
Write-Host "  Failed: $failed" -ForegroundColor $(if ($failed -gt 0) { "Red" } else { "Green" })

if ($failures.Count -gt 0) {
    Write-Host ""
    Write-Host "Failures:" -ForegroundColor Red
    foreach ($f in $failures) {
        Write-Host "  - $f" -ForegroundColor Red
    }
    Write-Host ""
    Write-Host "Root cause: psmux split-window returns exit 0 when it cannot" -ForegroundColor Yellow
    Write-Host "create a new pane (insufficient space). It should return a" -ForegroundColor Yellow
    Write-Host "non-zero exit code so callers can detect the failure." -ForegroundColor Yellow
}

exit $failed
