# Rails bench: a warm-adopted pane reports #{pane_ready} and satisfies --ready.
#
# Panes adopted from the warm pool used to get a fresh last_output_time Arc
# that nothing wrote: the warm reader thread was spawned with `None` for it
# (pane.rs spawn_warm_pane) and the adopting Pane built a new zeroed Arc. On
# such a pane #{pane_ready} stayed 0 forever and `wait-pane --ready` never
# fired. A plain split-window (no command, no -c, default shell) adopts the
# server's warm pane; that is the discriminating case. The first pane of a
# session takes the early warm pane only when the server was started with a
# size, so its case is a sanity check, not proof of the fix.
#
# Expected RED on the pre-fix binary: the split case prints 0 and the
# wait-pane case returns rc 1 after its full timeout. Not yet proven: this
# file has not been run (no psmux runs on the dev host).
. $PSScriptRoot/_harness.ps1

Write-Host "== warm pane readiness ==" -ForegroundColor Cyan
$S = New-IsolatedSession "warmready"
# Give the replacement warm pane time to spawn so split-window adopts it,
# and let the first pane fall silent (500 ms silence threshold).
Start-Sleep -Seconds 3

function Get-PaneReady {
    param([string]$Target)
    "$(psmux display-message -t $Target -p '#{pane_ready}' 2>$null)".Trim()
}

$first = "$(psmux list-panes -t $S -F '#{pane_id}' 2>$null | Select-Object -First 1)".Trim()
$FirstTarget = "${S}:.$first"

Test-Case "the session's first (warm) pane reports pane_ready=1 after silence" {
    $r = Get-PaneReady $FirstTarget
    if ($r -ne '1') { Write-Host "    pane_ready=$r" -ForegroundColor DarkYellow }
    $r -eq '1'
}

$null = psmux split-window -t $S 2>&1
Start-Sleep -Seconds 2
$second = "$(psmux list-panes -t $S -F '#{pane_id}' 2>$null | Select-Object -Last 1)".Trim()
$SecondTarget = "${S}:.$second"

Test-Case "a warm-adopted split pane reports pane_ready=1 after silence" {
    $r = Get-PaneReady $SecondTarget
    if ($r -ne '1') { Write-Host "    pane=$second pane_ready=$r" -ForegroundColor DarkYellow }
    $r -eq '1'
}

Test-Case "wait-pane --ready returns 0 promptly on a warm-adopted pane" {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $null = psmux wait-pane -t $SecondTarget --ready --timeout 5000 2>&1
    $rc = $LASTEXITCODE
    $sw.Stop()
    if ($rc -ne 0) { Write-Host "    rc=$rc after $($sw.ElapsedMilliseconds)ms" -ForegroundColor DarkYellow }
    ($rc -eq 0) -and ($sw.ElapsedMilliseconds -lt 3000)
}

Remove-PsmuxSession $S
Write-Summary
