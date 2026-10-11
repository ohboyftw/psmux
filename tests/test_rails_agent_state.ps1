# Rails bench: one readiness rule (Pane::readiness) behind #{pane_ready},
# #{pane_agent_source} and wait-pane --ready.
#
# A silent shell pane reports nothing, so it falls back to output silence:
# source=output_stable, ready=1, and wait-pane --ready returns 0. Once the
# pane's program reports `working` over OSC 7501, the reported state wins even
# though the pane is silent: source=reported, ready=0, wait-pane --ready times
# out. Reporting `idle` makes it ready again.
#
# Expected RED on the pre-03b binary: #{pane_agent_source} is not a known
# variable, so the source cases print an empty value, and a silent pane that
# reports `working` still shows pane_ready=1. Not yet proven: this file has
# not been run (no psmux runs on the dev host).
#
# Two things can break the reported cases without a psmux bug: ConPTY must
# pass OSC 7501 through to the pane's parser, and a shell prompt that emits
# OSC 133 A (shell integration in the pwsh profile) drops a `working` record
# at the next prompt, by protocol design.
. $PSScriptRoot/_harness.ps1

Write-Host "== agent state readiness ==" -ForegroundColor Cyan
$S = New-IsolatedSession "agentstate"
Start-Sleep -Seconds 3

$pane = "$(psmux list-panes -t $S -F '#{pane_id}' 2>$null | Select-Object -First 1)".Trim()
$T = "${S}:.$pane"

function Get-PaneVars {
    "$(psmux display-message -t $T -p '#{pane_agent_state}|#{pane_agent_source}|#{pane_ready}' 2>$null)".Trim()
}

function Send-ProgramStatus {
    param([string]$Body)
    $cmd = "[Console]::Write([char]27 + ']7501;$Body' + [char]27 + '\')"
    psmux send-keys -t $T $cmd Enter | Out-Null
    # Let the echo settle past the 500 ms silence window and one server tick.
    Start-Sleep -Milliseconds 1500
}

Test-Case "a silent shell pane is idle by output silence" {
    $v = Get-PaneVars
    if ($v -ne '|output_stable|1') { Write-Host "    state|source|ready=$v" -ForegroundColor DarkYellow }
    $v -eq '|output_stable|1'
}

Test-Case "wait-pane --ready agrees on the silent shell pane" {
    $null = psmux wait-pane -t $T --ready --timeout 3000 2>&1
    $rc = $LASTEXITCODE
    if ($rc -ne 0) { Write-Host "    rc=$rc" -ForegroundColor DarkYellow }
    $rc -eq 0
}

Send-ProgramStatus 'state=working'

Test-Case "a reported working state wins on a silent pane" {
    $v = Get-PaneVars
    if ($v -ne 'working|reported|0') { Write-Host "    state|source|ready=$v" -ForegroundColor DarkYellow }
    $v -eq 'working|reported|0'
}

Test-Case "wait-pane --ready times out while the pane reports working" {
    $null = psmux wait-pane -t $T --ready --timeout 1500 2>&1
    $rc = $LASTEXITCODE
    if ($rc -ne 1) { Write-Host "    rc=$rc" -ForegroundColor DarkYellow }
    $rc -eq 1
}

Send-ProgramStatus 'state=idle'

Test-Case "a reported idle state is ready" {
    $v = Get-PaneVars
    if ($v -ne 'idle|reported|1') { Write-Host "    state|source|ready=$v" -ForegroundColor DarkYellow }
    $v -eq 'idle|reported|1'
}

Send-ProgramStatus 'state=clear'

Test-Case "after state=clear the pane falls back to output silence" {
    $v = Get-PaneVars
    if ($v -ne '|output_stable|1') { Write-Host "    state|source|ready=$v" -ForegroundColor DarkYellow }
    $v -eq '|output_stable|1'
}

Remove-PsmuxSession $S
Write-Summary
