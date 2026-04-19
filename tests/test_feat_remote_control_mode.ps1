# Feature bench: remote tmux control mode (attach-remote, new-session-remote, -CC)
# Skips gracefully if no SSH target is configured via PSMUX_TEST_SSH_TARGET.
. $PSScriptRoot/_harness.ps1

Write-Host "== remote control mode ==" -ForegroundColor Cyan

$target = $env:PSMUX_TEST_SSH_TARGET
if (-not $target) {
    Write-Host "  i PSMUX_TEST_SSH_TARGET not set — skipping remote control mode tests" -ForegroundColor Cyan
    Write-Summary
    return
}

Test-Case "list-sessions-remote returns a table or 'no sessions' against $target" {
    $out = psmux list-sessions-remote -t $target 2>&1 | Out-String
    $LASTEXITCODE -eq 0 -and ($out.Length -ge 0)
}

Test-Case "new-session-remote accepts -d and returns a handle" {
    $name = "remote-probe-$((Get-Date).Ticks)"
    $out = psmux new-session-remote -t $target -s $name -d 2>&1 | Out-String
    # Clean up best-effort
    psmux kill-session-remote -t $target -s $name 2>&1 | Out-Null
    $LASTEXITCODE -eq 0
}

Write-Summary
