# Rails bench: wait-pane / wait-for --until STATE (ticket 05).
#
# --until idle is --ready (level-triggered), the other states need a reported
# OSC 7501 state, and a pane that dies before the state is seen answers rc 2
# instead of hanging or claiming success.
#
# Expected RED on the pre-05 binary: wait-pane ignores --until and waits for
# the child to exit (the idle case times out with rc 1 after the full 3 s
# window), and wait-for takes `blocked` as a channel name and blocks on it
# with no timeout, so prove RED for that case under an external timeout.
# Elapsed time, not only the rc, separates a correct verdict from those.
# Not yet proven: this file has not been run (no psmux runs on the dev host).
#
# The reported cases need OSC 7501 to reach the pane's parser through ConPTY,
# and a shell prompt that emits OSC 133 A drops a `working` record at the next
# prompt. If only those cases fail, check that first (see
# test_rails_agent_state.ps1).
. $PSScriptRoot/_harness.ps1

Write-Host "== wait --until agent state ==" -ForegroundColor Cyan
$S = New-IsolatedSession "waituntil"

$pane = "$(psmux list-panes -t $S -F '#{pane_id}' 2>$null | Select-Object -First 1)".Trim()
$T = "${S}:.$pane"

$deadline = (Get-Date).AddSeconds(20)
while ((Get-Date) -lt $deadline) {
    if ("$(psmux display-message -t $T -p '#{pane_ready}' 2>$null)".Trim() -eq '1') { break }
    Start-Sleep -Milliseconds 250
}

function Send-ProgramStatus {
    param([string]$Body)
    $cmd = "[Console]::Write([char]27 + ']7501;$Body' + [char]27 + '\')"
    psmux send-keys -t $T $cmd Enter | Out-Null
    Start-Sleep -Milliseconds 1500
}

function Measure-Wait {
    param([string[]]$WaitArgs)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $null = psmux @WaitArgs 2>&1
    $rc = $LASTEXITCODE
    $sw.Stop()
    return [PSCustomObject]@{ Rc = $rc; Ms = [int]$sw.ElapsedMilliseconds }
}

Test-Case "wait-pane --until idle on an idle pane returns rc 0 at once" {
    $r = Measure-Wait @('wait-pane', '-t', $T, '--until', 'idle', '--timeout', '3000')
    if ($r.Rc -ne 0 -or $r.Ms -ge 1500) { Write-Host "    rc=$($r.Rc) ms=$($r.Ms)" -ForegroundColor DarkYellow }
    ($r.Rc -eq 0) -and ($r.Ms -lt 1500)
}

Test-Case "wait-pane --until working times out on a silent, non-reporting pane" {
    $r = Measure-Wait @('wait-pane', '-t', $T, '--until', 'working', '--timeout', '1500')
    if ($r.Rc -ne 1 -or $r.Ms -lt 1400) { Write-Host "    rc=$($r.Rc) ms=$($r.Ms)" -ForegroundColor DarkYellow }
    ($r.Rc -eq 1) -and ($r.Ms -ge 1400)
}

Test-Case "wait-for --until blocked is not parsed as a channel name" {
    $r = Measure-Wait @('wait-for', '-t', $T, '--until', 'blocked', '--timeout', '1500')
    if ($r.Rc -ne 1 -or $r.Ms -ge 6000) { Write-Host "    rc=$($r.Rc) ms=$($r.Ms)" -ForegroundColor DarkYellow }
    ($r.Rc -eq 1) -and ($r.Ms -lt 6000)
}

Test-Case "an unknown --until state is rejected with rc 2" {
    $r = Measure-Wait @('wait-pane', '-t', $T, '--until', 'clear', '--timeout', '1500')
    $r.Rc -eq 2
}

Send-ProgramStatus 'state=working'

Test-Case "wait-pane --until working returns rc 0 once the pane reports working" {
    $r = Measure-Wait @('wait-pane', '-t', $T, '--until', 'working', '--timeout', '3000')
    if ($r.Rc -ne 0) { Write-Host "    rc=$($r.Rc) ms=$($r.Ms)" -ForegroundColor DarkYellow }
    $r.Rc -eq 0
}

Test-Case "wait-for --until working --json agrees with wait-pane" {
    $out = psmux wait-for -t $T --until working --timeout 3000 --json 2>$null
    $json = try { $out | ConvertFrom-Json } catch { $null }
    $json -and $json.kind -eq 'success'
}

Send-ProgramStatus 'id=t1:state=blocked:kind=question'

Test-Case "a blocked child record satisfies --until blocked under a working root" {
    $r = Measure-Wait @('wait-pane', '-t', $T, '--until', 'blocked', '--timeout', '3000')
    $r.Rc -eq 0
}

Send-ProgramStatus 'state=clear'
Send-ProgramStatus 'id=t1:state=clear'

$dying = "$(psmux split-window -t $S -d -P -F '#{pane_id}' -- pwsh -NoProfile -Command 'Start-Sleep -Seconds 2' 2>$null)".Trim()

Test-Case "wait-pane --until working on a pane that dies returns rc 2, not a hang" {
    if ($dying -notmatch '^%\d+$') { Write-Host "    split-window gave [$dying]" -ForegroundColor DarkYellow; return $false }
    $r = Measure-Wait @('wait-pane', '-t', "${S}:.$dying", '--until', 'working', '--timeout', '15000')
    if ($r.Rc -ne 2 -or $r.Ms -ge 10000) { Write-Host "    rc=$($r.Rc) ms=$($r.Ms)" -ForegroundColor DarkYellow }
    ($r.Rc -eq 2) -and ($r.Ms -lt 10000)
}

Test-Case "wait-pane --until on a pane that is already gone returns rc 2" {
    if ($dying -notmatch '^%\d+$') { return $false }
    $r = Measure-Wait @('wait-pane', '-t', "${S}:.$dying", '--until', 'idle', '--timeout', '3000')
    if ($r.Rc -ne 2) { Write-Host "    rc=$($r.Rc) ms=$($r.Ms)" -ForegroundColor DarkYellow }
    $r.Rc -eq 2
}

Test-Case "wait-for --until --json on a pane that is already gone prints kind=error, rc 2" {
    if ($dying -notmatch '^%\d+$') { return $false }
    $out = psmux wait-for -t "${S}:.$dying" --until idle --timeout 3000 --json 2>$null
    $rc = $LASTEXITCODE
    $json = try { $out | ConvertFrom-Json } catch { $null }
    if ($rc -ne 2 -or -not $json) { Write-Host "    rc=$rc out=[$out]" -ForegroundColor DarkYellow }
    ($rc -eq 2) -and $json -and ($json.kind -eq 'error')
}

Test-Case "wait-pane --until on a window.pane index target returns rc 2, not 1" {
    $r = Measure-Wait @('wait-pane', '-t', "${S}:0.0", '--until', 'idle', '--timeout', '3000')
    if ($r.Rc -ne 2) { Write-Host "    rc=$($r.Rc) ms=$($r.Ms)" -ForegroundColor DarkYellow }
    $r.Rc -eq 2
}

Test-Case "wait-for --until with --output is rejected with rc 2" {
    $r = Measure-Wait @('wait-for', '-t', $T, '--until', 'idle', '--output', 'x', '--timeout', '1500')
    $r.Rc -eq 2
}

Remove-PsmuxSession $S
Write-Summary
