# Rails bench: send-keys honours `--` as end-of-options.
#
# The operand filters selected keys with `!starts_with('-')`, which does not know
# where the flags stop, so a dash-leading payload was discarded — and `--` could
# not rescue it, because the marker was itself dropped as a dash token. The
# command exited 0 with nothing on stderr, so NO spelling delivered a
# dash-leading literal.
#
# Both halves are needed: the client parses flags too and would eat a payload of
# `-l`, and the server's `-t` prefilter runs before the send-keys arm, so a bare
# `-t` past the marker was consumed there along with the token after it.
. $PSScriptRoot/_harness.ps1

Write-Host "== send-keys -- end-of-options ==" -ForegroundColor Cyan
$S = New-IsolatedSession "sendkeys-endopts"
# One window per case: payloads sit on a prompt line and are never executed, so
# a shared window would concatenate them and blur the assertions.
foreach ($n in 1..3) { $null = psmux new-window -t $S 2>&1 }
Start-Sleep -Seconds 2

# Session-qualified pane ids, never a bare %N: a bare id routes to whatever
# ambient server answers first.
$Pane = @{}
foreach ($row in (psmux list-panes -a -t $S -F '#{window_index} #{pane_id}' 2>$null)) {
    $bits = "$row".Split(' ')
    if ($bits.Count -eq 2) { $Pane[[int]$bits[0]] = "${S}:.$($bits[1])" }
}

function Get-Text {
    param([string]$Target)
    return ((psmux capture-pane -p -t $Target 2>$null) -join "`n")
}

# Keys sent before the shell has drained its first paint are swallowed by
# ConPTY, and a swallowed key is indistinguishable from a dropped one — which is
# exactly what this file is testing for. So prove each pane accepts input first,
# by re-sending an ordinary payload until it echoes. `wait-pane --ready` is the
# purpose-built gate but returns timeout in ~130ms, so it cannot be used here.
function Send-Until {
    param([string]$Target, [string]$Text, [int]$TimeoutMs = 20000)
    $deadline = (Get-Date).AddMilliseconds($TimeoutMs)
    while ((Get-Date) -lt $deadline) {
        $null = psmux send-keys -t $Target -l $Text
        Start-Sleep -Milliseconds 600
        if ((Get-Text $Target) -match [regex]::Escape($Text)) { return $true }
    }
    return $false
}

$warm = @($Pane.Keys | Sort-Object | Where-Object { Send-Until $Pane[$_] "WARM$_-" })
Test-Case "every fixture pane accepts input before any case runs" {
    ($Pane.Count -eq 4) -and ($warm.Count -eq 4)
}

# One send per case from here on: a pass means that spelling delivered, not that
# a retry loop eventually got through.
$null = psmux send-keys -t $Pane[0] -l 'aSENTINEL-control'
$null = psmux send-keys -t $Pane[1] -l -- '-SENTINEL-dash'
# Separate tokens, not one quoted string: the prefilter compares whole args, so
# only a bare `-t` past the marker exercises it.
$null = psmux send-keys -t $Pane[2] -l -- '-t' 'SENTINEL-target'
$null = psmux send-keys -t $Pane[3] -l -- 'SENTINEL-marker'
Start-Sleep -Milliseconds 1500

Test-Case "control payload is delivered (probe is valid)" {
    (Get-Text $Pane[0]) -match 'aSENTINEL-control'
}

Test-Case "-- delivers a dash-leading literal" {
    $t = Get-Text $Pane[1]
    if ($t -notmatch '\-SENTINEL-dash') { Write-Host "    pane=[$t]" -ForegroundColor DarkYellow }
    $t -match '\-SENTINEL-dash'
}

Test-Case "-- protects a payload the -t prefilter would have eaten" {
    $t = Get-Text $Pane[2]
    if ($t -notmatch '\-t SENTINEL-target') { Write-Host "    pane=[$t]" -ForegroundColor DarkYellow }
    $t -match '\-t SENTINEL-target'
}

# Regression guard rather than a discriminator: the marker was already dropped
# before the fix, just for the wrong reason. It must not start being delivered.
Test-Case "the -- marker is consumed, not delivered as a key" {
    $t = Get-Text $Pane[3]
    ($t -match 'SENTINEL-marker') -and ($t -notmatch '\-\- ?SENTINEL-marker')
}

Test-Case "send-keys with no -- still delivers ordinary keys" {
    $null = psmux send-keys -t $Pane[0] -l 'SENTINEL-plain'
    Start-Sleep -Milliseconds 800
    (Get-Text $Pane[0]) -match 'SENTINEL-plain'
}

Remove-PsmuxSession $S
Write-Summary
