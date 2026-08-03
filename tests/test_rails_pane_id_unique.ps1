# Rails bench: pane ids must be unique across concurrently live sessions.
#
# next_pane_id starts at 1 in every AppState, so each server mints %1, %2, ...
# independently and ids collide across sessions. scan_servers_for_id resolves a
# bare `-t %N` by scanning servers, biased toward the caller's own session
# ("own session wins outright"). A script that reads a pane id from session B
# and issues `respawn-pane -k -t %N` while sitting in session A therefore hits
# A's pane instead of B's — and with remain-on-exit=off the pruned pane takes a
# single-pane session with it. That destroyed two live sessions on 2026-08-03.
#
# The bias is correct for Claude Code (it targets panes in its own session), so
# the fix is global uniqueness, not rejecting ambiguity.
. $PSScriptRoot/_harness.ps1

Write-Host "== pane id uniqueness ==" -ForegroundColor Cyan

Test-Case "pane ids are unique across concurrently live sessions" {
    $sessions = @()
    $ids = @()
    try {
        foreach ($i in 1..3) {
            $s = New-IsolatedSession "paneid-$i"
            $sessions += $s
            Start-Sleep -Milliseconds 900
            $ids += (psmux list-panes -s -t $s -F '#{pane_id}' 2>&1 |
                     Where-Object { $_ -match '^%' })
        }
        Write-Host ("    ids: {0}" -f ($ids -join ' ')) -ForegroundColor DarkGray
        $unique = ($ids | Select-Object -Unique)
        ($ids.Count -eq 3) -and ($unique.Count -eq $ids.Count)
    } finally {
        foreach ($s in $sessions) { Remove-PsmuxSession $s }
    }
}

# NOTE: an end-to-end case was tried here — drive session B's pane id from
# inside session A and assert A survives — and was REMOVED because it passed
# with the fix reverted. Two fresh sessions do not reliably collide (a run gave
# A=%1, B=%2), so the case only exercises the bug when the ids happen to match,
# which cannot be forced from outside. It could not fail, which makes it worse
# than no test. Uniqueness above is the real invariant: if no two live servers
# share an id, the misrouting is impossible by construction.

Write-Summary
