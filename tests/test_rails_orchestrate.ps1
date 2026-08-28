# Rails bench: psmux orchestrate (DAG runner)
. $PSScriptRoot/_harness.ps1

Write-Host "== orchestrate ==" -ForegroundColor Cyan

$workDir = Join-Path $env:TEMP "orc-$((Get-Date).Ticks)"
$planPath = Join-Path $workDir "plan.json"
New-Item -ItemType Directory -Path $workDir -Force | Out-Null
$session = "orc-$([guid]::NewGuid().Guid.Substring(0,6))"
$session2 = "orc-fail-$([guid]::NewGuid().Guid.Substring(0,6))"
$outA = Join-Path $workDir "a.out"
$outB = Join-Path $workDir "b.out"

Test-Case "plan.json with two sequential workers runs and both succeed" {
    # orchestrate requires the target session to exist (it calls new-window into it)
    psmux new-session -d -s $session 2>&1 | Out-Null
    Start-Sleep -Milliseconds 300
    $plan = @{
        version = 1
        session = $session
        workers = @(
            @{
                id = "a"
                cwd = $workDir
                command = @("cmd", "/c", "echo A > `"$outA`"")
                depends_on = @()
            }
            @{
                id = "b"
                cwd = $workDir
                command = @("cmd", "/c", "echo B > `"$outB`"")
                depends_on = @("a")
            }
        )
    } | ConvertTo-Json -Depth 6
    Set-Content -Path $planPath -Value $plan
    # orchestrate blocks until all workers finish; no --timeout flag — we rely
    # on the workers being trivial (echo + exit).
    $null = psmux orchestrate $planPath --timeout 15000 --json 2>&1
    Start-Sleep -Milliseconds 500
    (Test-Path $outA) -and (Test-Path $outB)
}

# The run leaves the session as it found it. Every worker's dead pane used to
# survive its own window: the cleanup killed the pane with a BARE `%id`, which
# resolves to nothing, so kill-pane exited 0 having done nothing and the result
# was discarded. A long-lived orchestrate session accumulated one window per
# worker, per run.
Test-Case "no worker window survives a completed run" {
    $windows = @(psmux list-windows -t $session 2>$null | ForEach-Object { [string]$_ })
    $leaked = @($windows | Where-Object { $_ -match '^\d+:\s*(a|b)[-*]?\s' })
    if ($leaked.Count -gt 0) {
        Write-Host "    leaked: $($leaked -join ' | ')" -ForegroundColor DarkYellow
    }
    $leaked.Count -eq 0
}

Test-Case "no dead pane survives a completed run" {
    $dead = @(psmux list-panes -a -t $session -F '#{pane_dead}' 2>$null |
        Where-Object { "$_".Trim() -eq '1' })
    $dead.Count -eq 0
}

Test-Case "state.json records both workers' exit codes as 0" {
    $stateCandidate = @(
        Join-Path $workDir ".orchestration/$session/state.json"
        Join-Path $env:LOCALAPPDATA "psmux/orchestration/$session/state.json"
        Join-Path $env:USERPROFILE ".psmux/orchestration/$session/state.json"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $stateCandidate) { return $false }
    $s = Get-Content $stateCandidate -Raw | ConvertFrom-Json
    ($s.workers.a.exit_code -eq 0) -and ($s.workers.b.exit_code -eq 0)
}

Test-Case "failure propagates: dependent worker is skipped" {
    $planPath2 = Join-Path $workDir "plan-fail.json"
    $outC = Join-Path $workDir "c.out"
    psmux new-session -d -s $session2 2>&1 | Out-Null
    Start-Sleep -Milliseconds 300
    $plan2 = @{
        version = 1
        session = $session2
        workers = @(
            @{ id = "fail"; cwd = $workDir; command = @("cmd", "/c", "exit 1"); depends_on = @() }
            @{
                id = "downstream"
                cwd = $workDir
                command = @("cmd", "/c", "echo C > `"$outC`"")
                depends_on = @("fail")
            }
        )
    } | ConvertTo-Json -Depth 6
    Set-Content -Path $planPath2 -Value $plan2
    $null = psmux orchestrate $planPath2 --json 2>&1
    Start-Sleep -Milliseconds 500
    -not (Test-Path $outC)
}

# Cleanup
psmux kill-session -t $session 2>&1 | Out-Null
psmux kill-session -t $session2 2>&1 | Out-Null
Remove-Item $workDir -Recurse -Force -ErrorAction SilentlyContinue
Write-Summary
