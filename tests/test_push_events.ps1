# Test: push events are delivered over named pipe with enriched fields
param([switch]$Verbose)

$ErrorActionPreference = "Stop"
$session = "test-events-$(Get-Random)"
$passed = 0; $failed = 0

function Assert($cond, $msg) {
    if ($cond) { $script:passed++; if ($Verbose) { Write-Host "  PASS: $msg" -Fore Green } }
    else { $script:failed++; Write-Host "  FAIL: $msg" -Fore Red }
}

try {
    # Create a session with a command that exits quickly
    psmux new-session -d -s $session
    Start-Sleep -Milliseconds 2000

    # Verify pane_dead_time is 0 for alive pane
    $deadTime = psmux display-message -t "${session}:" -p '#{pane_dead_time}'
    Assert ($deadTime -eq "0") "pane_dead_time should be 0 for alive pane (got: $deadTime)"

    # Verify pane_exit_code is empty for alive pane
    $exitCode = psmux display-message -t "${session}:" -p '#{pane_exit_code}'
    Assert ([string]::IsNullOrEmpty($exitCode)) "pane_exit_code should be empty for alive pane (got: $exitCode)"

    # Create a pane with a command that exits with known code
    psmux split-window -d -t "${session}:" --shell bash -- "exit 42"
    Start-Sleep -Milliseconds 2000

    # Check exit code is tracked
    $panes = psmux list-panes -t "${session}:" -F "#{pane_id} #{pane_dead} #{pane_exit_code} #{pane_dead_time}"
    if ($Verbose) { Write-Host "Panes: $panes" }

    # At least one pane should be dead with exit code 42
    $deadPanes = $panes -split "`n" | Where-Object { $_ -match "1 42" }
    Assert ($deadPanes.Count -ge 1) "Should have a dead pane with exit code 42"

    # Dead pane should have a real dead_time (non-zero timestamp)
    $deadWithTime = $panes -split "`n" | Where-Object {
        $parts = $_ -split ' '
        $parts.Count -ge 4 -and $parts[1] -eq "1" -and [int64]$parts[3] -gt 1000000000
    }
    Assert ($deadWithTime.Count -ge 1) "Dead pane should have real dead_time timestamp"

} finally {
    psmux kill-session -t $session 2>$null
    Write-Host "`nResults: $passed passed, $failed failed"
    if ($failed -gt 0) { exit 1 }
}
