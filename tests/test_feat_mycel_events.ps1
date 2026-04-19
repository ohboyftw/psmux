# Feature bench: mycel event bus (psmux/* topics)
# Requires psmux built with --features mycel and mycel server running.
. $PSScriptRoot/_harness.ps1

Write-Host "== mycel events ==" -ForegroundColor Cyan

# Probe whether mycel is wired into this build
$mycelProbe = psmux help 2>&1 | Out-String
$hasMycel = $mycelProbe -match 'mycel|event bus'
if (-not $hasMycel) {
    # Fallback probe: check for PSMUX_MYCEL env var hint or mycel binary on PATH
    $hasMycel = (Get-Command mycel -ErrorAction SilentlyContinue) -ne $null
}
if (-not $hasMycel) {
    Write-Host "  i mycel not detected (build without --features mycel or no CLI) — skipping" -ForegroundColor Cyan
    Write-Summary
    return
}

$S = New-IsolatedSession "mycel-events"

Test-Case "spawning a pane does not fail when mycel is active" {
    # Indirect: if mycel publish hooks panicked, the pane spawn would error.
    $before = (psmux list-panes -t $S 2>$null | Measure-Object -Line).Lines
    $null = psmux split-window -v -t $S 2>&1
    Start-Sleep -Milliseconds 400
    $after = (psmux list-panes -t $S 2>$null | Measure-Object -Line).Lines
    $after -gt $before
}

Test-Case "killing a pane does not fail when mycel publishes pane/exited" {
    $null = psmux kill-pane -t $S 2>&1
    Start-Sleep -Milliseconds 300
    $LASTEXITCODE -eq 0
}

Remove-PsmuxSession $S
Write-Summary
