# Rails bench: send-keys --wait-ready delivers to -t, not to the active pane.
#
# `send-keys -t %pane --wait-ready <payload>` delivered nothing to the target and
# exited 0, which read as a dropped payload. It was not dropped: it went to the
# wrong pane.
#
#   1. the `-t` prefilter sets a temporary focus (FocusPaneTempCheck)
#   2. --wait-ready then polls with CtrlReq::QueryPaneState
#   3. that poll is not a temp-focus request (is_temp_focus in server/mod.rs),
#      so the server treats it as "a command completed" and restores the
#      real focus
#   4. the following SendKeys goes to the active pane, which is now the
#      originally active one
#
# The fix (wait_ready_then_refocus in server/connection.rs) sends
# FocusPaneTempCheck again after the poll, so the keys follow the target.
#
# So the assertion that matters is not "did it arrive" but "did it arrive HERE
# and not THERE". A single-pane fixture cannot see this bug at all.
. $PSScriptRoot/_harness.ps1

Write-Host "== send-keys --wait-ready target routing ==" -ForegroundColor Cyan
$S = New-IsolatedSession "sendkeys-waitready"
$null = psmux new-window -t $S 2>&1
Start-Sleep -Seconds 2

# Session-qualified pane ids, not a bare %N. send-keys drops -t before routing,
# so a bare id goes to the caller's own session (or the most recently used one)
# and is never searched for across servers. A miss still exits 0.
# docs/scripting.md gives users the same advice.
$Pane = @{}
foreach ($row in (psmux list-panes -a -t $S -F '#{window_index} #{pane_id}' 2>$null)) {
    $bits = "$row".Split(' ')
    if ($bits.Count -eq 2) { $Pane[[int]$bits[0]] = "${S}:.$($bits[1])" }
}

# new-window leaves window 1 active. Window 0 must be the active pane so that a
# restore-to-active misroute is observable as "landed in 0 instead of 1".
$null = psmux select-window -t "${S}:0" 2>&1
Start-Sleep -Milliseconds 400

function Get-Text {
    param([string]$Target)
    return ((psmux capture-pane -p -t $Target 2>$null) -join "`n")
}

# Keys sent before the shell drains its first paint are swallowed by ConPTY, and
# a swallowed key looks exactly like a misrouted one. Prove each pane accepts
# input first. Plain send-keys (no --wait-ready) is used deliberately: it is the
# spelling known to route correctly.
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
Test-Case "both fixture panes accept input before any case runs" {
    ($Pane.Count -eq 2) -and ($warm.Count -eq 2)
}

# Re-assert window 0 as active: the warm-up above targeted window 1 last, and a
# plain -t send restores focus on completion, but make it explicit rather than
# depend on that restore.
$null = psmux select-window -t "${S}:0" 2>&1
Start-Sleep -Milliseconds 400

# --- case (a): the bug ------------------------------------------------------
# One send only. A pass here means this spelling routed correctly, not that a
# retry eventually got through.
$null = psmux send-keys -t $Pane[1] --wait-ready -l 'SENTINEL-wr'
Start-Sleep -Seconds 2
$w0 = Get-Text $Pane[0]
$w1 = Get-Text $Pane[1]

Test-Case "--wait-ready payload lands in the -t pane" {
    $w1 -match 'SENTINEL-wr'
}
Test-Case "--wait-ready payload does NOT land in the originally-active pane" {
    -not ($w0 -match 'SENTINEL-wr')
}

# --- case (b): regression guard ---------------------------------------------
# Targeting the pane that is already active must keep working. If a fix breaks
# this, it has traded one misroute for another.
$null = psmux send-keys -t $Pane[0] --wait-ready -l 'SENTINEL-self'
Start-Sleep -Seconds 2
Test-Case "--wait-ready to the already-active pane still delivers" {
    (Get-Text $Pane[0]) -match 'SENTINEL-self'
}

# --- case (c): composes with `--` end-of-options (T5e, 060388a) --------------
# A dash-leading payload past `--` must survive both the end-of-options handling
# and the readiness poll.
$null = psmux send-keys -t $Pane[1] --wait-ready -l -- '-SENTINEL-dash'
Start-Sleep -Seconds 2
Test-Case "--wait-ready composes with -- for a dash-leading payload" {
    (Get-Text $Pane[1]) -match '-SENTINEL-dash'
}

# NOTE: there is deliberately no "payload is withheld on timeout" case. The
# 30s deadline breaks the poll and delivers anyway (wait_ready_then_refocus),
# so non-delivery is never the contract.

Remove-PsmuxSession $S
Write-Summary
