# Rails bench: psmux orchestrate resumes from state.json
#
# Contract: if state.json shows A succeeded and B pending, re-running orchestrate:
#   - does NOT re-spawn A (started_at timestamp preserved)
#   - runs B to completion (exit 0)
#
# We simulate the "crash after A but before B" state by running the full plan,
# then manually resetting B's state back to pending. This is equivalent to what
# an actual process kill would leave (the poll loop is the only writer that
# advances B from pending→running→succeeded).
. $PSScriptRoot/_harness.ps1

Write-Host "== orchestrate resume-from-state.json ==" -ForegroundColor Cyan

$workDir  = Join-Path $env:TEMP "orc-resume-$((Get-Date).Ticks)"
$planPath = Join-Path $workDir "plan.json"
New-Item -ItemType Directory -Path $workDir -Force | Out-Null
$session   = "orc-resume-$([guid]::NewGuid().Guid.Substring(0,6))"
$outA      = Join-Path $workDir "a.done"
$outB      = Join-Path $workDir "b.done"
$stateFile = Join-Path $workDir ".orchestration/$session/state.json"

psmux new-session -d -s $session 2>&1 | Out-Null
Start-Sleep -Milliseconds 300

$plan = @{
    version = 1
    session = $session
    workers = @(
        @{
            id         = "a"
            cwd        = $workDir
            command    = @("cmd", "/c", "echo done > `"$outA`"")
            depends_on = @()
        }
        @{
            id         = "b"
            cwd        = $workDir
            command    = @("cmd", "/c", "echo done > `"$outB`"")
            depends_on = @("a")
        }
    )
} | ConvertTo-Json -Depth 6
Set-Content -Path $planPath -Value $plan

# Run the plan to completion (A and B both succeed)
$null = psmux orchestrate $planPath --timeout 15000 2>&1

Test-Case "baseline: both workers succeeded in first full run" {
    if (-not (Test-Path $stateFile)) { return $false }
    $s = Get-Content $stateFile -Raw | ConvertFrom-Json
    $s.workers.a.status -eq "succeeded" -and $s.workers.b.status -eq "succeeded"
}

# Capture A's started_at — must be unchanged after resume
$startedAtA = $null
if (Test-Path $stateFile) {
    $snap = Get-Content $stateFile -Raw | ConvertFrom-Json
    $startedAtA = $snap.workers.a.started_at
}

# Simulate crash: reset B to pending and delete its output so the re-run is meaningful
if (Test-Path $stateFile) {
    $s = Get-Content $stateFile -Raw | ConvertFrom-Json
    $s.workers.b.status    = "pending"
    $s.workers.b.exit_code = $null
    $s.workers.b.pane_id   = $null
    $s.workers.b.started_at  = $null
    $s.workers.b.finished_at = $null
    $s | ConvertTo-Json -Depth 6 | Set-Content -Path $stateFile
}
Remove-Item $outB -Force -ErrorAction SilentlyContinue

# Resume run
$null = psmux orchestrate $planPath --timeout 15000 2>&1
$ec2 = $LASTEXITCODE

Test-Case "resume: second pass exits 0 (all workers succeeded)" {
    $ec2 -eq 0
}

Test-Case "resume: B output file exists after second pass" {
    Test-Path $outB
}

Test-Case "resume: A's started_at is unchanged (was not re-spawned)" {
    if (-not (Test-Path $stateFile)) { return $false }
    $s2 = Get-Content $stateFile -Raw | ConvertFrom-Json
    $startedAtA -and ($s2.workers.a.started_at -eq $startedAtA)
}

Test-Case "resume: state.json shows both workers succeeded" {
    if (-not (Test-Path $stateFile)) { return $false }
    $s2 = Get-Content $stateFile -Raw | ConvertFrom-Json
    $s2.workers.a.status -eq "succeeded" -and $s2.workers.b.status -eq "succeeded"
}

# Cleanup
psmux kill-session -t $session 2>&1 | Out-Null
Remove-Item $workDir -Recurse -Force -ErrorAction SilentlyContinue
Write-Summary
