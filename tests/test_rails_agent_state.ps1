# Rails bench: one readiness rule (Pane::readiness) behind #{pane_ready},
# #{pane_agent_readiness}, #{pane_agent_source} and wait-pane --ready.
#
# A silent shell pane reports nothing, so it falls back to output silence:
# readiness=idle, source=output_stable, ready=1, and wait-pane --ready
# returns 0. Once the
# pane's program reports `working` over OSC 7501, the reported state wins even
# though the pane is silent: source=reported, ready=0, wait-pane --ready times
# out. Reporting `idle` makes it ready again.
#
# Expected RED on the pre-03b binary: #{pane_agent_readiness} and
# #{pane_agent_source} are not known variables, so every case prints empty
# values, and a silent pane that reports `working` still shows pane_ready=1. Not yet proven: this file has
# not been run (no psmux runs on the dev host).
#
# Two things can break the reported cases without a psmux bug: ConPTY must
# pass OSC 7501 through to the pane's parser, and a shell prompt that emits
# OSC 133 A (shell integration in the pwsh profile) drops a `working` record
# at the next prompt, by protocol design. A failure of only the reported
# cases most likely means OSC 7501 never reaches the parser. The file probes
# for that once and SKIPS the reported cases with a visible message instead of
# failing, so it only fails on psmux bugs.
. $PSScriptRoot/_harness.ps1

Write-Host "== agent state readiness ==" -ForegroundColor Cyan
$S = New-IsolatedSession "agentstate"

$pane = "$(psmux list-panes -t $S -F '#{pane_id}' 2>$null | Select-Object -First 1)".Trim()
$T = "${S}:.$pane"

# Wait for the shell to finish its start-up output instead of a fixed sleep.
$deadline = (Get-Date).AddSeconds(20)
while ((Get-Date) -lt $deadline) {
    if ("$(psmux display-message -t $T -p '#{pane_ready}' 2>$null)".Trim() -eq '1') { break }
    Start-Sleep -Milliseconds 250
}

function Get-PaneVars {
    "$(psmux display-message -t $T -p '#{pane_agent_readiness}|#{pane_agent_source}|#{pane_ready}' 2>$null)".Trim()
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
    if ($v -ne 'idle|output_stable|1') { Write-Host "    readiness|source|ready=$v" -ForegroundColor DarkYellow }
    $v -eq 'idle|output_stable|1'
}

Test-Case "wait-pane --ready agrees on the silent shell pane" {
    $null = psmux wait-pane -t $T --ready --timeout 3000 2>&1
    $rc = $LASTEXITCODE
    if ($rc -ne 0) { Write-Host "    rc=$rc" -ForegroundColor DarkYellow }
    $rc -eq 0
}

# Probe once. The reported cases may only be skipped when ConPTY is the one
# thing at fault, so first prove the format variables work and that a control
# OSC (title, OSC 2) which ConPTY does forward reaches the parser in this same
# pane. If either fails, that is a failure, not a skip. Only when both hold and
# OSC 7501 still does not register is it recorded as passthrough-dropped.
$vars = Get-PaneVars
Test-Case "agent-state format variables resolve (probe precondition)" {
    if ($vars -notmatch '^[a-z]+\|[a-z_]+\|[01]$') { Write-Host "    readiness|source|ready='$vars'" -ForegroundColor DarkYellow }
    $vars -match '^[a-z]+\|[a-z_]+\|[01]$'
}

psmux send-keys -t $T "[Console]::Write([char]27 + ']2;railsctl' + [char]27 + '\')" Enter | Out-Null
Start-Sleep -Milliseconds 1500
$title = "$(psmux display-message -t $T -p '#{pane_title}' 2>$null)".Trim()
Test-Case "a control OSC (title) reaches the parser (probe precondition)" {
    if ($title -ne 'railsctl') { Write-Host "    pane_title='$title'" -ForegroundColor DarkYellow }
    $title -eq 'railsctl'
}

Send-ProgramStatus 'state=working'

$probe = Get-PaneVars
$preconditionsHold = ($vars -match '^[a-z]+\|[a-z_]+\|[01]$') -and ($title -eq 'railsctl')
if ($preconditionsHold -and $probe.Split('|')[1] -ne 'reported') {
    Write-Host "  SKIPPED: reported-state cases (ConPTY passed the title OSC but OSC 7501 did not register; probe saw '$probe')" -ForegroundColor Yellow
    Write-Host "::warning::test_rails_agent_state: reported-state cases SKIPPED, OSC 7501 not passed through on this host"
} elseif (-not $preconditionsHold) {
    Write-Host "  reported-state cases not attempted: probe preconditions failed (see above)" -ForegroundColor Red
} else {
    Test-Case "a reported working state wins on a silent pane" {
        $v = Get-PaneVars
        if ($v -ne 'working|reported|0') { Write-Host "    readiness|source|ready=$v" -ForegroundColor DarkYellow }
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
        if ($v -ne 'idle|reported|1') { Write-Host "    readiness|source|ready=$v" -ForegroundColor DarkYellow }
        $v -eq 'idle|reported|1'
    }

    Send-ProgramStatus 'state=clear'

    Test-Case "after state=clear the pane falls back to output silence" {
        $v = Get-PaneVars
        if ($v -ne 'idle|output_stable|1') { Write-Host "    readiness|source|ready=$v" -ForegroundColor DarkYellow }
        $v -eq 'idle|output_stable|1'
    }
}

Remove-PsmuxSession $S
Write-Summary
