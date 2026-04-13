# Test: exec with -t targeting runs in the correct pane's context
param([switch]$Verbose)

$ErrorActionPreference = "Stop"
$session = "test-exec-target-$(Get-Random)"
$passed = 0; $failed = 0

function Assert($cond, $msg) {
    if ($cond) { $script:passed++; if ($Verbose) { Write-Host "  PASS: $msg" -Fore Green } }
    else { $script:failed++; Write-Host "  FAIL: $msg" -Fore Red }
}

try {
    # Setup: create session with a pane that has a known cwd
    psmux new-session -d -s $session
    Start-Sleep -Milliseconds 1500

    # Create a second pane with a specific working directory
    $tempDir = Join-Path $env:TEMP "psmux-exec-test-$(Get-Random)"
    New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
    psmux split-window -h -d -t "${session}:" -c $tempDir

    Start-Sleep -Milliseconds 1500

    # Get pane IDs
    $panes = psmux list-panes -t "${session}:" -F "#{pane_id}" 2>&1
    $paneIds = $panes -split "`n" | Where-Object { $_ -match '^%\d+$' }
    Assert ($paneIds.Count -ge 2) "Should have at least 2 panes (got $($paneIds.Count))"

    # Exec in the second pane — verify it runs in the tempDir
    $secondPane = $paneIds[1]
    $result = psmux exec -t $secondPane -- pwd
    Assert ($result -match [regex]::Escape($tempDir) -or $result -match "psmux-exec-test") `
        "exec -t should use target pane's cwd (got: $result)"

    # Exec without -t should use active pane (first pane)
    $defaultResult = psmux exec -- echo hello
    Assert ($defaultResult -match "hello") "exec without -t should work (got: $defaultResult)"

} finally {
    psmux kill-session -t $session 2>$null
    if ($tempDir -and (Test-Path $tempDir)) { Remove-Item $tempDir -Recurse -Force }
    Write-Host "`nResults: $passed passed, $failed failed"
    if ($failed -gt 0) { exit 1 }
}
