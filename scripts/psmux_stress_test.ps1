# scripts/psmux_stress_test.ps1
#
# PowerShell stress test runner for psmux on Windows.
# Runs outside of Rust to catch OS-level resource leaks (handles, processes, ports).
#
# Usage:
#   .\psmux_stress_test.ps1 -TestSuite All
#   .\psmux_stress_test.ps1 -TestSuite MemoryLeak
#   .\psmux_stress_test.ps1 -TestSuite CpuRunoff
#   .\psmux_stress_test.ps1 -TestSuite HandleLeak
#   .\psmux_stress_test.ps1 -TestSuite OrphanProcess

param(
    [ValidateSet("All", "MemoryLeak", "CpuRunoff", "HandleLeak", "OrphanProcess")]
    [string]$TestSuite = "All",
    [int]$Cycles = 100,
    [string]$OutputJson = "psmux_stress_results.json"
)

$ErrorActionPreference = "Stop"
$results = @()

function Write-TestHeader($name) {
    Write-Host "`n$('='*60)" -ForegroundColor Cyan
    Write-Host "  TEST: $name" -ForegroundColor Cyan
    Write-Host "$('='*60)" -ForegroundColor Cyan
}

function Write-TestResult($name, $passed, $details) {
    $color = if ($passed) { "Green" } else { "Red" }
    $status = if ($passed) { "PASS" } else { "FAIL" }
    Write-Host "  [$status] $name" -ForegroundColor $color
    foreach ($key in $details.Keys) {
        Write-Host "    $key = $($details[$key])" -ForegroundColor Gray
    }
    $script:results += @{
        name = $name
        passed = $passed
        details = $details
        timestamp = (Get-Date -Format "o")
    }
}

function Cleanup {
    $null = & psmux kill-server 2>&1
    Start-Sleep -Seconds 1
}

function Get-PsmuxProcess {
    Get-Process -Name "psmux" -ErrorAction SilentlyContinue | Select-Object -First 1
}

function Get-PsmuxMemoryMB {
    $proc = Get-PsmuxProcess
    if ($proc) { [math]::Round($proc.WorkingSet64 / 1MB, 2) } else { 0 }
}

function Get-PsmuxHandleCount {
    $proc = Get-PsmuxProcess
    if ($proc) { $proc.HandleCount } else { 0 }
}

function Get-PsmuxCpuTime {
    $proc = Get-PsmuxProcess
    if ($proc) { $proc.TotalProcessorTime.TotalSeconds } else { 0 }
}

# ─── Memory Leak Test ───────────────────────────────────────────────────────

function Test-MemoryLeak {
    Write-TestHeader "Memory Leak - Session Lifecycle ($Cycles cycles)"
    Cleanup

    # Baseline
    $null = & psmux new-session -d -s baseline 2>&1
    Start-Sleep -Seconds 2
    $baselineMB = Get-PsmuxMemoryMB
    Write-Host "  Baseline memory: ${baselineMB}MB"

    # Stress
    for ($i = 0; $i -lt $Cycles; $i++) {
        $null = & psmux new-session -d -s "leak-$i" 2>&1
        $null = & psmux new-window -t "leak-$i" 2>&1
        $null = & psmux split-window -t "leak-$i" 2>&1
        $null = & psmux kill-session -t "leak-$i" 2>&1

        if ($i % 25 -eq 0) {
            $currentMB = Get-PsmuxMemoryMB
            Write-Host "  Cycle $i/$Cycles - Memory: ${currentMB}MB" -ForegroundColor Gray
        }
    }

    Start-Sleep -Seconds 5
    $finalMB = Get-PsmuxMemoryMB
    $deltaMB = [math]::Round($finalMB - $baselineMB, 2)
    $passed = $deltaMB -lt 10  # 10MB threshold

    Write-TestResult "memory_leak_session_lifecycle" $passed @{
        baseline_mb = $baselineMB
        final_mb = $finalMB
        delta_mb = $deltaMB
        cycles = $Cycles
        threshold_mb = 10
    }
    Cleanup
}

# ─── CPU Runoff Test ────────────────────────────────────────────────────────

function Test-CpuRunoff {
    Write-TestHeader "CPU Runoff - Concurrent Output Flood"
    Cleanup

    $null = & psmux new-session -d -s flood 2>&1

    # Create 8 panes
    for ($i = 0; $i -lt 7; $i++) {
        $null = & psmux split-window -t flood 2>&1
        $null = & psmux select-layout -t flood tiled 2>&1
    }

    # Record CPU before flood
    $cpuBefore = Get-PsmuxCpuTime

    # Start flood in all panes
    for ($i = 0; $i -lt 8; $i++) {
        $null = & psmux send-keys -t "flood:.$i" `
            'cmd /c "for /L %x in (1,1,99999) do @echo FLOOD"' Enter 2>&1
    }

    # Monitor for 15 seconds
    $cpuSamples = @()
    for ($s = 0; $s -lt 15; $s++) {
        Start-Sleep -Seconds 1
        $proc = Get-PsmuxProcess
        if ($proc) {
            $cpuSamples += $proc.CPU
        }
    }

    # Stop flood
    for ($i = 0; $i -lt 8; $i++) {
        $null = & psmux send-keys -t "flood:.$i" C-c "" 2>&1
    }

    $cpuAfter = Get-PsmuxCpuTime
    $cpuUsed = [math]::Round($cpuAfter - $cpuBefore, 2)
    $avgCpu = if ($cpuSamples.Count -gt 0) {
        [math]::Round(($cpuSamples | Measure-Object -Average).Average, 2)
    } else { 0 }

    # Test responsiveness during flood
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $null = & psmux list-sessions 2>&1
    $sw.Stop()
    $responsiveMs = $sw.ElapsedMilliseconds

    $passed = ($responsiveMs -lt 2000)

    Write-TestResult "cpu_runoff_concurrent_flood" $passed @{
        cpu_time_seconds = $cpuUsed
        avg_cpu_metric = $avgCpu
        responsiveness_ms = $responsiveMs
        pane_count = 8
    }
    Cleanup
}

# ─── Handle Leak Test ───────────────────────────────────────────────────────

function Test-HandleLeak {
    Write-TestHeader "Handle Leak - ConPTY Create/Destroy ($Cycles cycles)"
    Cleanup

    $null = & psmux new-session -d -s handle-baseline 2>&1
    Start-Sleep -Seconds 2
    $baselineHandles = Get-PsmuxHandleCount
    Write-Host "  Baseline handles: $baselineHandles"

    for ($i = 0; $i -lt $Cycles; $i++) {
        $null = & psmux new-session -d -s "handle-$i" 2>&1
        $null = & psmux split-window -t "handle-$i" 2>&1
        $null = & psmux split-window -t "handle-$i" 2>&1
        Start-Sleep -Milliseconds 50
        $null = & psmux kill-session -t "handle-$i" 2>&1
        Start-Sleep -Milliseconds 100

        if ($i % 25 -eq 0) {
            $currentHandles = Get-PsmuxHandleCount
            Write-Host "  Cycle $i/$Cycles - Handles: $currentHandles" -ForegroundColor Gray
        }
    }

    Start-Sleep -Seconds 5
    $finalHandles = Get-PsmuxHandleCount
    $leaked = $finalHandles - $baselineHandles
    $passed = $leaked -lt 20  # allow small handle variance

    Write-TestResult "handle_leak_conpty" $passed @{
        baseline_handles = $baselineHandles
        final_handles = $finalHandles
        leaked_handles = $leaked
        cycles = $Cycles
        threshold = 20
    }
    Cleanup
}

# ─── Orphan Process Test ────────────────────────────────────────────────────

function Test-OrphanProcess {
    Write-TestHeader "Orphan Process - Shell Cleanup ($Cycles cycles)"
    Cleanup

    # Count shell processes before
    $shellsBefore = @(Get-Process -Name "powershell","pwsh","cmd" -ErrorAction SilentlyContinue).Count
    Write-Host "  Shell processes before: $shellsBefore"

    for ($i = 0; $i -lt $Cycles; $i++) {
        $null = & psmux new-session -d -s "orphan-$i" 2>&1
        $null = & psmux kill-session -t "orphan-$i" 2>&1

        if ($i % 25 -eq 0) {
            $current = @(Get-Process -Name "powershell","pwsh","cmd" -ErrorAction SilentlyContinue).Count
            Write-Host "  Cycle $i/$Cycles - Shell processes: $current" -ForegroundColor Gray
        }
    }

    Start-Sleep -Seconds 5
    Cleanup
    Start-Sleep -Seconds 3

    $shellsAfter = @(Get-Process -Name "powershell","pwsh","cmd" -ErrorAction SilentlyContinue).Count
    $orphans = $shellsAfter - $shellsBefore
    $passed = $orphans -le 2  # small tolerance for background processes

    Write-TestResult "orphan_process_shells" $passed @{
        shells_before = $shellsBefore
        shells_after = $shellsAfter
        orphans = $orphans
        cycles = $Cycles
        threshold = 2
    }
}

# ─── Main ───────────────────────────────────────────────────────────────────

Write-Host "`n╔══════════════════════════════════════════════╗" -ForegroundColor Yellow
Write-Host "║   psmux Stress Test Suite                     ║" -ForegroundColor Yellow
Write-Host "║   Suite: $TestSuite | Cycles: $Cycles              ║" -ForegroundColor Yellow
Write-Host "╚══════════════════════════════════════════════╝" -ForegroundColor Yellow

switch ($TestSuite) {
    "All" {
        Test-MemoryLeak
        Test-CpuRunoff
        Test-HandleLeak
        Test-OrphanProcess
    }
    "MemoryLeak"    { Test-MemoryLeak }
    "CpuRunoff"     { Test-CpuRunoff }
    "HandleLeak"    { Test-HandleLeak }
    "OrphanProcess" { Test-OrphanProcess }
}

# ─── Summary ────────────────────────────────────────────────────────────────

Write-Host "`n$('='*60)" -ForegroundColor Yellow
Write-Host "  SUMMARY" -ForegroundColor Yellow
Write-Host "$('='*60)" -ForegroundColor Yellow

$passed = ($results | Where-Object { $_.passed }).Count
$failed = ($results | Where-Object { -not $_.passed }).Count
$total = $results.Count

foreach ($r in $results) {
    $color = if ($r.passed) { "Green" } else { "Red" }
    $status = if ($r.passed) { "PASS" } else { "FAIL" }
    Write-Host "  [$status] $($r.name)" -ForegroundColor $color
}

Write-Host "`n  Total: $total | Passed: $passed | Failed: $failed" -ForegroundColor $(
    if ($failed -eq 0) { "Green" } else { "Red" }
)

# Export JSON results
$results | ConvertTo-Json -Depth 5 | Out-File -FilePath $OutputJson -Encoding UTF8
Write-Host "`n  Results saved to: $OutputJson" -ForegroundColor Gray

exit $failed
