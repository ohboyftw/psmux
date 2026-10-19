# Rails bench: window targets resolve to the window they name (psmux/psmux#627).
#
# Measured on ohboy-builds 727a8ab5, windows @1 "w1" and @2 "w2" (split):
#
#   display-message -t alpha:@2.%N  -> can't find window, exit 1
#   display-message -t @2           -> answered for w1, exit 0
#   display-message -t alpha:@2     -> answered for w1, exit 0
#   display-message -t alpha:w2     -> can't find window, exit 1
#   split-window -d -t alpha:w2     -> split w1, exit 0
#
# The server read every window number as an INDEX, so @2 meant index 2; with
# two windows no such index exists and the command fell back to the active
# window. A window name was never looked up at all.
. $PSScriptRoot/_harness.ps1

Write-Host "== window target resolution (#627) ==" -ForegroundColor Cyan
$S = "wtr-" + [guid]::NewGuid().Guid.Substring(0, 8)
$null = psmux new-session -d -s $S -n w1 2>&1
Start-Sleep -Milliseconds 400
$null = psmux new-window -d -t $S -n w2 2>&1
Start-Sleep -Milliseconds 300

function Get-Windows {
    $rows = @(psmux list-windows -t $S -F '#{window_index} #{window_id} #{window_name}' 2>$null |
        Where-Object { $_ -ne '' })
    foreach ($r in $rows) {
        $p = $r.Trim() -split '\s+'
        [PSCustomObject]@{ Index = $p[0]; Id = $p[1]; Name = $p[2] }
    }
}

function Get-PaneCount([string]$WindowId) {
    @(psmux list-panes -s -t $S -F '#{window_id} #{pane_id}' 2>$null |
        Where-Object { $_ -ne '' -and ($_.Trim() -split '\s+')[0] -eq $WindowId }).Count
}

function Show([string]$Target) {
    $out = psmux display-message -p -t $Target '#{session_name}/#{window_id}/#{window_name}/#{pane_id}' 2>&1 |
        Out-String
    [PSCustomObject]@{ Out = $out.Trim(); Rc = $LASTEXITCODE }
}

$wins = @(Get-Windows)
$w1 = $wins | Where-Object Name -eq 'w1'
$w2 = $wins | Where-Object Name -eq 'w2'

# Split w2 by index, a form that already worked, so the fixture does not
# depend on the code under test.
$null = psmux split-window -d -t "${S}:$($w2.Index)" 2>&1
Start-Sleep -Milliseconds 400
$w2Pane = @(psmux list-panes -s -t $S -F '#{window_id} #{pane_id}' 2>$null |
    Where-Object { $_ -ne '' -and ($_.Trim() -split '\s+')[0] -eq $w2.Id } |
    ForEach-Object { ($_.Trim() -split '\s+')[1] }) | Select-Object -Last 1

Test-Case "fixture: windows w1 and w2 with distinct ids, w2 has two panes" {
    $null -ne $w1 -and $null -ne $w2 -and $w1.Id -ne $w2.Id -and (Get-PaneCount $w2.Id) -eq 2
}

$w2Prefix = "$S/$($w2.Id)/w2/"

Test-Case "display-message -t <session>:@id.%pane answers for that pane" {
    $r = Show "${S}:$($w2.Id).$w2Pane"
    if ($r.Out -ne "$w2Prefix$w2Pane") { Write-Host "    got: $($r.Out) rc=$($r.Rc)" -ForegroundColor DarkYellow }
    $r.Rc -eq 0 -and $r.Out -eq "$w2Prefix$w2Pane"
}

Test-Case "display-message -t <session>:@id answers for that window, not the active one" {
    $r = Show "${S}:$($w2.Id)"
    if (-not $r.Out.StartsWith($w2Prefix)) { Write-Host "    got: $($r.Out) rc=$($r.Rc)" -ForegroundColor DarkYellow }
    $r.Rc -eq 0 -and $r.Out.StartsWith($w2Prefix)
}

Test-Case "display-message -t @id (no session) answers for that window" {
    # A bare @id is routed by scanning servers; PSMUX_SESSION biases the scan
    # to this one, so another live server with the same id cannot answer.
    $env:PSMUX_SESSION = $S
    try { $r = Show $w2.Id } finally { $env:PSMUX_SESSION = $null }
    if (-not $r.Out.StartsWith($w2Prefix)) { Write-Host "    got: $($r.Out) rc=$($r.Rc)" -ForegroundColor DarkYellow }
    $r.Rc -eq 0 -and $r.Out.StartsWith($w2Prefix)
}

Test-Case "display-message -t <session>:<name> answers for that window" {
    $r = Show "${S}:w2"
    if (-not $r.Out.StartsWith($w2Prefix)) { Write-Host "    got: $($r.Out) rc=$($r.Rc)" -ForegroundColor DarkYellow }
    $r.Rc -eq 0 -and $r.Out.StartsWith($w2Prefix)
}

Test-Case "display-message -t <session>:<name>.%pane answers for that pane" {
    $r = Show "${S}:w2.$w2Pane"
    $r.Rc -eq 0 -and $r.Out -eq "$w2Prefix$w2Pane"
}

Test-Case "display-message -t <session>:<unknown name> exits non-zero" {
    (Show "${S}:nosuchwindow").Rc -ne 0
}

Test-Case "display-message -t <session>:@<unknown id> exits non-zero" {
    (Show "${S}:@99999999").Rc -ne 0
}

Test-Case "display-message -t <session>:<unknown index> exits non-zero" {
    (Show "${S}:99").Rc -ne 0
}

Test-Case "display-message -t <session>:@id.%pane in ANOTHER window exits non-zero" {
    (Show "${S}:$($w1.Id).$w2Pane").Rc -ne 0
}

Test-Case "split-window -d -t <session>:<name> splits that window and no other" {
    $before1 = Get-PaneCount $w1.Id
    $before2 = Get-PaneCount $w2.Id
    $null = psmux split-window -d -t "${S}:w2" 2>&1
    $rc = $LASTEXITCODE
    Start-Sleep -Milliseconds 400
    $after1 = Get-PaneCount $w1.Id
    $after2 = Get-PaneCount $w2.Id
    if ($after2 -ne $before2 + 1) { Write-Host "    w1 $before1->$after1  w2 $before2->$after2 rc=$rc" -ForegroundColor DarkYellow }
    $rc -eq 0 -and $after1 -eq $before1 -and $after2 -eq $before2 + 1
}

Test-Case "split-window -d -t <session>:@id splits that window and no other" {
    $before1 = Get-PaneCount $w1.Id
    $before2 = Get-PaneCount $w2.Id
    $null = psmux split-window -d -t "${S}:$($w2.Id)" 2>&1
    $rc = $LASTEXITCODE
    Start-Sleep -Milliseconds 400
    $rc -eq 0 -and (Get-PaneCount $w1.Id) -eq $before1 -and (Get-PaneCount $w2.Id) -eq $before2 + 1
}

Test-Case "split-window -t <session>:<unknown name> exits non-zero and splits nothing" {
    $before1 = Get-PaneCount $w1.Id
    $null = psmux split-window -d -t "${S}:nosuchwindow" 2>&1
    $rc = $LASTEXITCODE
    Start-Sleep -Milliseconds 300
    $rc -ne 0 -and (Get-PaneCount $w1.Id) -eq $before1
}

Test-Case "send-keys -t <session>:<unknown name> exits non-zero" {
    $null = psmux send-keys -t "${S}:nosuchwindow" 'echo hi' 2>&1
    $LASTEXITCODE -ne 0
}

Test-Case "select-window -t <session>:<name> selects that window" {
    $null = psmux select-window -t "${S}:w2" 2>&1
    $rc = $LASTEXITCODE
    Start-Sleep -Milliseconds 200
    $active = (psmux display-message -p -t $S '#{window_name}' 2>$null | Out-String).Trim()
    $rc -eq 0 -and $active -eq 'w2'
}

Test-Case "select-window -t <session>:@id selects that window" {
    $null = psmux select-window -t "${S}:$($w1.Id)" 2>&1
    $rc = $LASTEXITCODE
    Start-Sleep -Milliseconds 200
    $active = (psmux display-message -p -t $S '#{window_name}' 2>$null | Out-String).Trim()
    $rc -eq 0 -and $active -eq 'w1'
}

Remove-PsmuxSession $S
Write-Summary
