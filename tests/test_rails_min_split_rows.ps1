# Rails bench: MIN_SPLIT_ROWS invariant (P1.12 — ohboy uses 3 not 2
# because default pane-border-status="top" steals 1 row per pane)
. $PSScriptRoot/_harness.ps1

Write-Host "== MIN_SPLIT_ROWS ==" -ForegroundColor Cyan

# Spawn a detached session with a tiny initial height so we can probe the limit.
# We can't force exact rows from the CLI reliably on Windows, so we test the
# server-side check indirectly: attempt splits on a real session and inspect
# the resulting pane count / error text.
$S = New-IsolatedSession "min-split"

Test-Case "split-window -v on a normally-sized pane succeeds" {
    $before = (psmux list-panes -t $S 2>$null | Measure-Object -Line).Lines
    $null = psmux split-window -v -t $S 2>&1
    Start-Sleep -Milliseconds 300
    $after = (psmux list-panes -t $S 2>$null | Measure-Object -Line).Lines
    $after -gt $before
}

Test-Case "split-window -h on a normally-sized pane succeeds" {
    $before = (psmux list-panes -t $S 2>$null | Measure-Object -Line).Lines
    $null = psmux split-window -h -t $S 2>&1
    Start-Sleep -Milliseconds 300
    $after = (psmux list-panes -t $S 2>$null | Measure-Object -Line).Lines
    $after -gt $before
}

Test-Case "repeated vertical splits eventually fail with a 'too small' error (MIN_SPLIT_ROWS guard)" {
    # Kill + recreate so we start fresh
    Remove-PsmuxSession $S
    $script:S = New-IsolatedSession "min-split-tight"
    $rejected = $false
    for ($i = 0; $i -lt 10; $i++) {
        $err = psmux split-window -v -t $script:S 2>&1 | Out-String
        if ($err -match 'too small|pane too small') {
            $rejected = $true
            break
        }
    }
    $rejected
}

Remove-PsmuxSession $script:S -ErrorAction SilentlyContinue
Write-Summary
