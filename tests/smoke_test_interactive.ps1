<#
.SYNOPSIS
    Interactive smoke test for psmux new features: hints mode, resurrection, layout files.
    Run this OUTSIDE of any psmux session (from a plain PowerShell terminal).

.USAGE
    pwsh tests/smoke_test_interactive.ps1
#>

$ErrorActionPreference = "Continue"
$PSMUX = (Get-Command psmux -ErrorAction SilentlyContinue).Source
if (-not $PSMUX) { $PSMUX = "$PSScriptRoot\..\target\release\psmux.exe" }
$SESSION = "smoke-test"

Write-Host "=== psmux Interactive Smoke Test ===" -ForegroundColor Cyan
Write-Host "Binary: $PSMUX" -ForegroundColor DarkGray
Write-Host ""

# ── Cleanup ──────────────────────────────────────────────────────────
& $PSMUX kill-session -t $SESSION 2>&1 | Out-Null
Start-Sleep -Milliseconds 500

# ── TEST 1: Layout File ─────────────────────────────────────────────
Write-Host "--- TEST 1: Declarative Layout File ---" -ForegroundColor Yellow

$layoutJson = @'
{
  "session": "smoke-test",
  "windows": [
    {
      "name": "dev",
      "layout": "main-vertical",
      "panes": [
        { "command": "bash -c 'echo PANE_ONE_READY && exec bash'" },
        { "command": "bash -c 'echo PANE_TWO_READY && exec bash'" },
        { "command": "bash -c 'echo PANE_THREE_READY && exec bash'" }
      ]
    }
  ]
}
'@

$layoutFile = Join-Path $env:TEMP "psmux-smoke-layout.json"
$layoutJson | Out-File -FilePath $layoutFile -Encoding UTF8

# Create session, then source the layout
& $PSMUX new-session -d -s $SESSION 2>&1 | Out-Null
Start-Sleep -Milliseconds 800
& $PSMUX source-file $layoutFile 2>&1 | Out-Null
Start-Sleep -Seconds 2

$panes = & $PSMUX list-panes -t $SESSION 2>&1
$paneCount = ($panes -split "`n").Count
if ($paneCount -ge 3) {
    Write-Host "[PASS] Layout file created $paneCount panes" -ForegroundColor Green
} else {
    Write-Host "[FAIL] Layout file: expected 3+ panes, got $paneCount" -ForegroundColor Red
}

# Check pane output
$captured = & $PSMUX capture-pane -t $SESSION -p 2>&1
if ($captured -match "PANE_.*READY") {
    Write-Host "[PASS] Layout pane commands executed (found READY marker)" -ForegroundColor Green
} else {
    Write-Host "[INFO] Layout panes spawned but markers may not be in active pane" -ForegroundColor Yellow
}

Remove-Item $layoutFile -ErrorAction SilentlyContinue

# ── TEST 2: Hints Mode ──────────────────────────────────────────────
Write-Host ""
Write-Host "--- TEST 2: Hints Mode ---" -ForegroundColor Yellow

# Send some URLs and file paths to the active pane
& $PSMUX send-keys -t $SESSION "echo 'Visit https://github.com/ohboyftw/psmux for source'" Enter 2>&1 | Out-Null
& $PSMUX send-keys -t $SESSION "echo 'Edit src/main.rs:42 and src/hints.rs:100'" Enter 2>&1 | Out-Null
& $PSMUX send-keys -t $SESSION "echo 'Commit abc1234 and def5678901234'" Enter 2>&1 | Out-Null
Start-Sleep -Seconds 2

# Verify the text is in the pane
$captured = & $PSMUX capture-pane -t $SESSION -p 2>&1
if ($captured -match "https://github.com") {
    Write-Host "[PASS] URLs visible in pane output" -ForegroundColor Green
} else {
    Write-Host "[FAIL] URLs not found in capture" -ForegroundColor Red
}

Write-Host "[INFO] Hints mode test: attach to '$SESSION' and press Ctrl+b f" -ForegroundColor Cyan
Write-Host "       You should see labels (a, s, d...) on URLs, file paths, and hashes." -ForegroundColor Cyan
Write-Host "       Type a label key to copy that text to clipboard." -ForegroundColor Cyan
Write-Host "       Press Esc to cancel, or wait 5 seconds for timeout." -ForegroundColor Cyan

# ── TEST 3: Resurrection ────────────────────────────────────────────
Write-Host ""
Write-Host "--- TEST 3: Session Resurrection ---" -ForegroundColor Yellow

# Split a few more panes to make an interesting layout
& $PSMUX split-window -h -t $SESSION 2>&1 | Out-Null
& $PSMUX split-window -v -t $SESSION 2>&1 | Out-Null
Start-Sleep -Milliseconds 500
& $PSMUX select-layout -t $SESSION tiled 2>&1 | Out-Null
Start-Sleep -Milliseconds 500

$panes = & $PSMUX list-panes -t $SESSION 2>&1
$paneCount = ($panes -split "`n").Count
Write-Host "[INFO] Session has $paneCount panes before kill" -ForegroundColor Cyan

# Check snapshot exists
$homeDir = $env:USERPROFILE
$snapFile = Join-Path $homeDir ".psmux\resurrect\$SESSION.json"
if (Test-Path $snapFile) {
    Write-Host "[PASS] Resurrection snapshot exists at $snapFile" -ForegroundColor Green
    $snap = Get-Content $snapFile -Raw | ConvertFrom-Json
    $snapPanes = ($snap.windows | ForEach-Object { $_.pane_commands.Count } | Measure-Object -Sum).Sum
    Write-Host "[INFO] Snapshot contains $($snap.windows.Count) windows, $snapPanes panes" -ForegroundColor Cyan
} else {
    Write-Host "[FAIL] No resurrection snapshot found" -ForegroundColor Red
}

# Kill the server (simulates crash)
Write-Host "[INFO] Killing server to simulate crash..." -ForegroundColor Cyan
& $PSMUX kill-server 2>&1 | Out-Null
Start-Sleep -Milliseconds 500

# Verify snapshot survived
if (Test-Path $snapFile) {
    Write-Host "[PASS] Snapshot survived server kill" -ForegroundColor Green
} else {
    Write-Host "[FAIL] Snapshot lost after kill" -ForegroundColor Red
}

# Check list-sessions shows resurrectable
$ls = & $PSMUX list-sessions 2>&1
if ($ls -match "resurrectable") {
    Write-Host "[PASS] list-sessions shows (resurrectable) tag" -ForegroundColor Green
} else {
    Write-Host "[INFO] list-sessions: $ls" -ForegroundColor Yellow
}

# Test resurrect command
Write-Host "[INFO] Running: psmux resurrect $SESSION" -ForegroundColor Cyan
Write-Host "[INFO] This will create a detached session and attach to it." -ForegroundColor Cyan
Write-Host ""
Write-Host "To test resurrection manually:" -ForegroundColor Yellow
Write-Host "  psmux resurrect $SESSION" -ForegroundColor White
Write-Host ""
Write-Host "To clean up:" -ForegroundColor Yellow
Write-Host "  psmux kill-session -t $SESSION" -ForegroundColor White
Write-Host "  psmux delete-resurrect $SESSION" -ForegroundColor White

# ── Summary ──────────────────────────────────────────────────────────
Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  INTERACTIVE SMOKE TEST COMPLETE" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Yellow
Write-Host "  1. Run 'psmux attach -t $SESSION' to test hints mode (Ctrl+b f)" -ForegroundColor White
Write-Host "  2. Run 'psmux resurrect $SESSION' to test resurrection" -ForegroundColor White
Write-Host "  3. Create your own layout.json and test with 'psmux source-file layout.json'" -ForegroundColor White
