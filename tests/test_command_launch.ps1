# Test: new-window and split-window -- command launches the command as initial process
param([switch]$Verbose)

$ErrorActionPreference = "Stop"
$session = "test-cmdlaunch-$(Get-Random)"
$passed = 0; $failed = 0

function Assert($cond, $msg) {
    if ($cond) { $script:passed++; if ($Verbose) { Write-Host "  PASS: $msg" -Fore Green } }
    else { $script:failed++; Write-Host "  FAIL: $msg" -Fore Red }
}

try {
    psmux new-session -d -s $session
    Start-Sleep -Milliseconds 1500

    # Test 1: new-window -- command that echoes and exits
    psmux new-window -d -t "${session}:" --shell bash -- "echo MARKER_CMD_LAUNCH && sleep 2"
    Start-Sleep -Milliseconds 2000

    # Capture output — should contain the marker
    $panes = psmux list-panes -t "${session}:" -F "#{pane_id}"
    $lastPane = ($panes -split "`n" | Where-Object { $_ -match '^%\d+$' })[-1]
    $output = psmux capture-pane -t $lastPane -p
    Assert ($output -match "MARKER_CMD_LAUNCH") "new-window -- command should run the command (got output)"

    # Test 2: split-window -- command
    psmux split-window -d -h -t "${session}:" --shell bash -- "echo SPLIT_MARKER && sleep 2"
    Start-Sleep -Milliseconds 2000

    $panes2 = psmux list-panes -t "${session}:" -F "#{pane_id}"
    $lastPane2 = ($panes2 -split "`n" | Where-Object { $_ -match '^%\d+$' })[-1]
    $output2 = psmux capture-pane -t $lastPane2 -p
    Assert ($output2 -match "SPLIT_MARKER") "split-window -- command should run the command"

    # Test 3: -- command with --shell override
    psmux new-window -d -t "${session}:" --shell bash -- "echo SHELL_OVERRIDE"
    Start-Sleep -Milliseconds 2000

    $panes3 = psmux list-panes -t "${session}:" -F "#{pane_id}"
    $lastPane3 = ($panes3 -split "`n" | Where-Object { $_ -match '^%\d+$' })[-1]
    $output3 = psmux capture-pane -t $lastPane3 -p
    Assert ($output3 -match "SHELL_OVERRIDE") "-- command with --shell should work"

} finally {
    psmux kill-session -t $session 2>$null
    Write-Host "`nResults: $passed passed, $failed failed"
    if ($failed -gt 0) { exit 1 }
}
