# Rails bench: psmux orchestrate --timeout fires exit code 3
. $PSScriptRoot/_harness.ps1

Write-Host "== orchestrate --timeout exit-code-3 ==" -ForegroundColor Cyan

$workDir = Join-Path $env:TEMP "orc-tmo-$((Get-Date).Ticks)"
$planPath = Join-Path $workDir "plan.json"
New-Item -ItemType Directory -Path $workDir -Force | Out-Null
$session = "orc-tmo-$([guid]::NewGuid().Guid.Substring(0,6))"

psmux new-session -d -s $session 2>&1 | Out-Null
Start-Sleep -Milliseconds 300

$plan = @{
    version = 1
    session = $session
    workers = @(
        @{
            id         = "blocker"
            cwd        = $workDir
            # 60-second ping will never complete within the 4-second timeout
            command    = @("cmd", "/c", "ping", "-n", "60", "127.0.0.1")
            depends_on = @()
        }
    )
} | ConvertTo-Json -Depth 6
Set-Content -Path $planPath -Value $plan

Test-Case "orchestrate --timeout fires: exit code 3" {
    $t0 = Get-Date
    $null = psmux orchestrate $planPath --timeout 4000 --json 2>&1
    $ec = $LASTEXITCODE
    $elapsed = ((Get-Date) - $t0).TotalMilliseconds
    # Must exit with 3 (timeout sentinel) within 8 seconds (4s ceiling + overhead)
    ($ec -eq 3) -and ($elapsed -lt 8000)
}

Test-Case "state.json marks blocker as failed with exit_code -2 after timeout" {
    $stateFile = Join-Path $workDir ".orchestration/$session/state.json"
    if (-not (Test-Path $stateFile)) { return $false }
    $s = Get-Content $stateFile -Raw | ConvertFrom-Json
    # exit_code -2 is the sentinel psmux uses for "timed out by orchestrator"
    $s.workers.blocker.status -eq "failed" -and $s.workers.blocker.exit_code -eq -2
}

# Cleanup
psmux kill-session -t $session 2>&1 | Out-Null
Remove-Item $workDir -Recurse -Force -ErrorAction SilentlyContinue
Write-Summary
