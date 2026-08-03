# Rails bench: respawn-pane must report failure through its EXIT CODE.
#
# Claude Code's teammate launcher does:
#   respawn-pane -k -t %N -- "cd <dir> && env VAR=VAL claude --agent-id ..."
#   if (n.code !== 0) throw
# Today the CLI is fire-and-forget (send_control drops the server's reply, and the
# respawn-pane arm never writes one), so a command that dies on spawn still exits 0.
# CC then reports "Spawned successfully" and waits forever on a process that never
# started.
#
# ISOLATION: every case gets its OWN session. A failing respawn kills the pane, and
# with remain-on-exit=off the pane is pruned — taking the (single-pane) session with
# it. Sharing one session across cases makes later cases assert "unknown session"
# instead of the contract under test.
#
# TARGETS: session-name (-t $S), never a bare pane-id (-t %N). Pane-ids are
# per-server counters that collide across sessions, and scan_servers_for_id takes
# the FIRST match, so a bare %N can resolve to a different session entirely.
#
# SHELL: `sleep 30` is used for the success case because it is valid in both bash
# and pwsh (where `sleep` aliases Start-Sleep) — the pane shell depends on
# default-shell, which is machine-specific.
. $PSScriptRoot/_harness.ps1

Write-Host "== respawn-pane exit codes ==" -ForegroundColor Cyan

Test-Case "respawn-pane -k exits non-zero when the command cannot execute" {
    $S = New-IsolatedSession "respawn-rc-fail"
    Start-Sleep -Milliseconds 800
    $null = psmux respawn-pane -k -t $S -- "definitely-not-a-real-binary-zz9" 2>&1
    $rc = $LASTEXITCODE
    Start-Sleep -Milliseconds 800
    Remove-PsmuxSession $S
    $rc -ne 0
}

Test-Case "respawn-pane without -k on a live pane exits non-zero" {
    $S = New-IsolatedSession "respawn-rc-nok"
    Start-Sleep -Milliseconds 800
    $null = psmux respawn-pane -t $S 2>&1
    $rc = $LASTEXITCODE
    Remove-PsmuxSession $S
    $rc -ne 0
}

Test-Case "respawn-pane exits non-zero for an unknown session target" {
    $null = psmux respawn-pane -k -t "no-such-session-zz9" -- "echo hi" 2>&1
    $LASTEXITCODE -ne 0
}

Test-Case "respawn-pane -k exits zero when the command starts successfully" {
    $S = New-IsolatedSession "respawn-rc-ok"
    Start-Sleep -Milliseconds 800
    $null = psmux respawn-pane -k -t $S -- "sleep 30" 2>&1
    $rc = $LASTEXITCODE
    Start-Sleep -Milliseconds 800
    # Prove the pane really is alive, so a future "always exit 0" regression
    # cannot pass this case by accident.
    $dead = (psmux display-message -t $S -p '#{pane_dead}' 2>$null).Trim()
    Remove-PsmuxSession $S
    ($rc -eq 0) -and ($dead -eq '0')
}

Write-Summary
