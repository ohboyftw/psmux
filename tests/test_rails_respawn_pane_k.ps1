# Rails bench: respawn-pane -k wired end-to-end (P1.9, upstream 88daff3)
# NOTE: CLI is fire-and-forget (send_control), so we assert behavior (PID change)
# rather than exit codes. Server-side rejection is the real contract.
# Targets use session-name (-t $S), not pane-id (-t %N) — pane-ids are per-server
# and scan across multiple live servers can be ambiguous (see session-checkpoint
# 2026-04-18). Session-name routing is unambiguous via direct .port lookup.
. $PSScriptRoot/_harness.ps1

Write-Host "== respawn-pane -k ==" -ForegroundColor Cyan
$S = New-IsolatedSession "respawn-k"
Start-Sleep -Milliseconds 800

function Get-PanePid {
    param([string]$Target)
    (psmux display-message -t $Target -p '#{pane_pid}' 2>$null).Trim()
}

Test-Case "bare respawn-pane on a live pane does NOT restart it (PID unchanged)" {
    $oldPid = Get-PanePid -Target $S
    $null = psmux respawn-pane -t $S 2>&1
    Start-Sleep -Milliseconds 600
    $newPid = Get-PanePid -Target $S
    # Server must reject; PID must stay the same
    $oldPid -eq $newPid
}

Test-Case "respawn-pane -k on a live pane restarts it (PID changes)" {
    $oldPid = Get-PanePid -Target $S
    $null = psmux respawn-pane -k -t $S 2>&1
    Start-Sleep -Milliseconds 1500
    $newPid = Get-PanePid -Target $S
    $newPid -and ($newPid -match '^\d+$') -and ($oldPid -ne $newPid)
}

# tmux syntax is `respawn-pane [-k] [-t target] [shell-command]`. Claude Code's
# teammate launcher issues `respawn-pane -k -t %N -- <command>`; dropping the
# command silently respawned the default shell instead, so the teammate never
# started and the leader waited on it forever.
Test-Case "respawn-pane -k with a shell-command runs the command, not the shell" {
    $null = psmux respawn-pane -k -t $S -- "Write-Host RAILS_RESPAWN_CMD_OK; Start-Sleep 30" 2>&1
    Wait-ForOutput -Target $S -Pattern 'RAILS_RESPAWN_CMD_OK' -TimeoutMs 8000
}

Remove-PsmuxSession $S
Write-Summary
