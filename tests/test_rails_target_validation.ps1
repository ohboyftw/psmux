# Rails bench: -t targets are honoured, and an unresolvable one fails loudly.
#
# Every case here used to pass silently against the WRONG target and exit 0.
# The display-message case is the one that was caught live: two distinct panes
# both reported the active pane's pid, which sent two investigations chasing
# the wrong process.
. $PSScriptRoot/_harness.ps1

Write-Host "== -t target validation ==" -ForegroundColor Cyan
$S = New-IsolatedSession "target-valid"
Start-Sleep -Milliseconds 300
$null = psmux split-window -t $S 2>&1
Start-Sleep -Milliseconds 400

# list-panes honours -t correctly and is the reference the rest is checked against.
$rows = @(psmux list-panes -t $S -F '#{pane_id} #{pane_pid}' 2>$null | Where-Object { $_ -ne '' })
$panes = @{}
foreach ($r in $rows) {
    $parts = $r.Trim() -split '\s+'
    if ($parts.Count -ge 2) { $panes[$parts[0]] = $parts[1] }
}

Test-Case "the fixture has two distinct panes with distinct pids" {
    $panes.Count -eq 2 -and (($panes.Values | Select-Object -Unique).Count -eq 2)
}

Test-Case "display-message -t %id reports THAT pane's pid, not the active one" {
    $ok = $true
    foreach ($id in $panes.Keys) {
        $got = (psmux display-message -t $id -p '#{pane_pid}' 2>$null).Trim()
        if ($got -ne $panes[$id]) {
            Write-Host ("    $id expected $($panes[$id]) got $got") -ForegroundColor DarkYellow
            $ok = $false
        }
    }
    $ok
}

# These are session-qualified on purpose. A bare "%N" carries no session, so it
# routes to whatever server the ambient environment resolves to — which under a
# test runner is some other session entirely, and the assertion then measures
# that server instead of this one.
Test-Case "display-message -t <session>:.%nonexistent exits non-zero" {
    $null = psmux display-message -t "${S}:.%99999999" -p '#{pane_pid}' 2>&1
    $LASTEXITCODE -ne 0
}

Test-Case "display-message -t <session>:.%malformed exits non-zero" {
    $null = psmux display-message -t "${S}:.%notanumber" -p '#{pane_pid}' 2>&1
    $LASTEXITCODE -ne 0
}

Test-Case "select-window -t <session>:<name> exits non-zero (no window-name resolution)" {
    $null = psmux select-window -t "${S}:nosuchwindow" 2>&1
    $LASTEXITCODE -ne 0
}

Test-Case "kill-window -t <session>:<name> fails instead of killing the active window" {
    $before = @(psmux list-windows -t $S 2>$null | Where-Object { $_ -ne '' }).Count
    $null = psmux kill-window -t "${S}:nosuchwindow" 2>&1
    $failed = $LASTEXITCODE -ne 0
    Start-Sleep -Milliseconds 200
    $after = @(psmux list-windows -t $S 2>$null | Where-Object { $_ -ne '' }).Count
    $failed -and ($after -eq $before)
}

Test-Case "the = exact-match prefix is accepted, not treated as part of the name" {
    $got = (psmux display-message -t "=$S" -p '#{session_name}' 2>$null).Trim()
    $LASTEXITCODE -eq 0 -and $got -eq $S
}

# Guard for the spin bug: the "can't find pane" path used to `continue` without
# clearing the command buffer, and the loop head only reads a new command when
# that buffer is empty — so the server re-ran the command forever and the client
# grew its reply buffer without bound. `send-keys` is fire-and-forget, so it
# still exits 0 here; RETURNING AT ALL is the property under test. Bounded by a
# job so a regression fails this case instead of hanging the whole bench.
Test-Case "send-keys -t <dead pane> returns instead of spinning the server" {
    $job = Start-Job -ScriptBlock {
        param($t)
        $null = psmux send-keys -t $t 'hello' 2>&1
    } -ArgumentList "${S}:.%99999999"
    $finished = Wait-Job $job -Timeout 15
    Stop-Job $job -ErrorAction SilentlyContinue
    Remove-Job $job -Force -ErrorAction SilentlyContinue
    $null -ne $finished
}

Test-Case "a well-formed -t still works and exits 0" {
    $id = @($panes.Keys)[0]
    $got = (psmux display-message -t $id -p '#{pane_id}' 2>$null).Trim()
    $LASTEXITCODE -eq 0 -and $got -eq $id
}

Remove-PsmuxSession $S
Write-Summary
