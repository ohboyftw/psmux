# test_scroll_memory.ps1 — Automated memory leak regression test
#
# Reproduces the copy-mode scroll memory leak by:
#   1. Starting a detached psmux session with PSMUX_MEMORY_DEBUG=1
#   2. Filling the pane with scrollback content
#   3. Injecting rapid scroll-up events (triggers copy mode entry)
#   4. Sampling process memory at intervals
#   5. Asserting memory stays within bounds
#
# Usage:  pwsh tests/test_scroll_memory.ps1 [-ScrollCount 2000] [-MemoryLimitMB 500]

param(
    [int]$ScrollCount   = 2000,    # total scroll events to inject
    [int]$MemoryLimitMB = 500,     # fail if server exceeds this
    [int]$BurstSize     = 50,      # events per burst
    [int]$BurstDelayMs  = 10,      # ms between events within a burst
    [int]$PauseMs       = 200      # ms pause between bursts (lets server process)
)

$ErrorActionPreference = "Continue"
$script:TestsPassed = 0
$script:TestsFailed = 0

function Write-Pass { param($msg) Write-Host "[PASS] $msg" -ForegroundColor Green; $script:TestsPassed++ }
function Write-Fail { param($msg) Write-Host "[FAIL] $msg" -ForegroundColor Red; $script:TestsFailed++ }
function Write-Info { param($msg) Write-Host "[INFO] $msg" -ForegroundColor Cyan }
function Write-Test { param($msg) Write-Host "[TEST] $msg" -ForegroundColor White }

# ── Resolve binary ──────────────────────────────────────────────────────────

$PSMUX = (Resolve-Path "$PSScriptRoot\..\target\release\psmux.exe" -ErrorAction SilentlyContinue).Path
if (-not $PSMUX) {
    $PSMUX = (Resolve-Path "$PSScriptRoot\..\target\debug\psmux.exe" -ErrorAction SilentlyContinue).Path
}
if (-not $PSMUX) {
    Write-Error "psmux binary not found — run 'cargo build --release' first"
    exit 1
}

Write-Info "Binary: $PSMUX"

$SESSION = "mem-leak-test"
$PSMUX_DIR = "$env:USERPROFILE\.psmux"

# ── Helper: get server PID ──────────────────────────────────────────────────

function Get-ServerPid {
    # Find the psmux server for our session by matching the TCP port it listens on
    $portFile = "$PSMUX_DIR\$SESSION.port"
    if (!(Test-Path $portFile)) { return $null }
    $sessionPort = [int](Get-Content $portFile)
    # Find PID listening on that port via netstat
    $listener = netstat -ano 2>$null | Select-String "127\.0\.0\.1:$sessionPort\s" |
        Select-String "LISTENING" | Select-Object -First 1
    if ($listener) {
        $parts = ($listener.ToString().Trim()) -split '\s+'
        $foundPid = [int]$parts[-1]
        return Get-Process -Id $foundPid -ErrorAction SilentlyContinue
    }
    # Fallback: newest psmux process (exclude our own PID)
    return Get-Process psmux -ErrorAction SilentlyContinue |
        Where-Object { $_.Id -ne $PID } |
        Sort-Object StartTime -Descending |
        Select-Object -First 1
}

function Get-MemoryMB {
    param([int]$ProcessId)
    if ($ProcessId -eq 0) { return 0 }
    # Fresh lookup each time — Refresh() is unreliable on Windows
    $p = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($null -eq $p) { return 0 }
    return [math]::Round($p.WorkingSet64 / 1MB, 1)
}

# ── Cleanup from prior runs ────────────────────────────────────────────────

Write-Info "Cleaning up prior test sessions..."
& $PSMUX kill-session -t $SESSION 2>$null | Out-Null
Start-Sleep -Seconds 1

# ── Start session ───────────────────────────────────────────────────────────

Write-Test "Starting detached session '$SESSION' with PSMUX_MEMORY_DEBUG=1"

$env:PSMUX_MEMORY_DEBUG = "1"
Start-Process -FilePath $PSMUX -ArgumentList "new-session -s $SESSION -d" -WindowStyle Hidden
Start-Sleep -Seconds 4

# Verify session exists
& $PSMUX has-session -t $SESSION 2>$null
if ($LASTEXITCODE -ne 0) {
    Write-Fail "Session '$SESSION' failed to start"
    exit 1
}
Write-Pass "Session started"

# ── Get server process ──────────────────────────────────────────────────────

$serverProc = Get-ServerPid
if ($null -eq $serverProc) {
    Write-Fail "Could not find server process"
    & $PSMUX kill-session -t $SESSION 2>$null
    exit 1
}
$serverPid = $serverProc.Id
$baselineMB = Get-MemoryMB $serverPid
Write-Info "Server PID: $serverPid, baseline memory: ${baselineMB} MB"

# ── Fill scrollback ────────────────────────────────────────────────────────

Write-Test "Filling scrollback buffer with content..."
# Generate ~1000 lines so there's content to scroll through
for ($i = 0; $i -lt 10; $i++) {
    & $PSMUX send-keys -t $SESSION "seq 1 100" Enter 2>$null | Out-Null
    Start-Sleep -Milliseconds 300
}
Start-Sleep -Seconds 2
Write-Pass "Scrollback populated"

# ── Connect TCP for scroll injection ────────────────────────────────────────

$portFile = "$PSMUX_DIR\$SESSION.port"
$keyFile  = "$PSMUX_DIR\$SESSION.key"

if (!(Test-Path $portFile) -or !(Test-Path $keyFile)) {
    Write-Fail "Port/key files not found for session '$SESSION'"
    & $PSMUX kill-session -t $SESSION 2>$null
    exit 1
}

$port = [int](Get-Content $portFile)
$key  = (Get-Content $keyFile).Trim()

Write-Info "Connecting to 127.0.0.1:$port for scroll injection..."
$tcp = [System.Net.Sockets.TcpClient]::new()
$tcp.NoDelay = $true
try {
    $tcp.Connect("127.0.0.1", $port)
} catch {
    Write-Fail "TCP connection failed: $_"
    & $PSMUX kill-session -t $SESSION 2>$null
    exit 1
}

$stream = $tcp.GetStream()
$writer = [System.IO.StreamWriter]::new($stream)
$writer.AutoFlush = $true

# Auth + persistent mode
$writer.WriteLine("AUTH $key")
$writer.WriteLine("PERSISTENT")
Start-Sleep -Milliseconds 200

Write-Pass "TCP connected and authenticated"

# ── Inject scroll events in bursts ──────────────────────────────────────────

Write-Test "Injecting $ScrollCount scroll-up events (burst=$BurstSize, delay=${BurstDelayMs}ms)..."

$memorySamples = @()
$sent = 0
$burstNum = 0

# Record baseline
$memorySamples += [PSCustomObject]@{
    Events = 0
    MemoryMB = $baselineMB
    Timestamp = (Get-Date)
}

while ($sent -lt $ScrollCount) {
    $burstNum++
    $thisBurst = [math]::Min($BurstSize, $ScrollCount - $sent)

    for ($i = 0; $i -lt $thisBurst; $i++) {
        try {
            $writer.WriteLine("scroll-up 40 20")
        } catch {
            Write-Fail "TCP write failed at event $sent : $_"
            break
        }
        $sent++
        if ($BurstDelayMs -gt 0) {
            Start-Sleep -Milliseconds $BurstDelayMs
        }
    }

    # Sample memory after each burst
    $currentMB = Get-MemoryMB $serverPid
    $memorySamples += [PSCustomObject]@{
        Events = $sent
        MemoryMB = $currentMB
        Timestamp = (Get-Date)
    }

    # Early abort if already way over limit
    if ($currentMB -gt ($MemoryLimitMB * 2)) {
        Write-Fail "EARLY ABORT: memory at ${currentMB} MB after $sent events (limit: $MemoryLimitMB MB)"
        break
    }

    if ($burstNum % 5 -eq 0) {
        Write-Info "  $sent/$ScrollCount events sent — server at ${currentMB} MB"
    }

    if ($PauseMs -gt 0) {
        Start-Sleep -Milliseconds $PauseMs
    }
}

# Final settle
Start-Sleep -Seconds 2
$finalMB = Get-MemoryMB $serverPid
$memorySamples += [PSCustomObject]@{
    Events = $sent
    MemoryMB = $finalMB
    Timestamp = (Get-Date)
}

Write-Info "Injection complete: $sent events sent"

# ── Close TCP ───────────────────────────────────────────────────────────────

try { $tcp.Close() } catch {}

# ── Verify copy mode was entered ────────────────────────────────────────────

Write-Test "Verifying copy mode was triggered..."
$inMode = & $PSMUX display-message -t $SESSION -p '#{pane_in_mode}' 2>$null
if ($inMode -match "1") {
    Write-Pass "Pane entered copy mode (as expected from scroll injection)"
} else {
    Write-Info "Pane not in copy mode (may have auto-exited) — mode=$inMode"
}

# ── Memory analysis ─────────────────────────────────────────────────────────

Write-Test "Analyzing memory growth..."

$peakMB = ($memorySamples | Measure-Object -Property MemoryMB -Maximum).Maximum
$growthMB = [math]::Round($finalMB - $baselineMB, 1)
$duration = ($memorySamples[-1].Timestamp - $memorySamples[0].Timestamp).TotalSeconds
$growthRate = if ($duration -gt 0) { [math]::Round($growthMB / $duration, 1) } else { 0 }

Write-Info "  Baseline:    ${baselineMB} MB"
Write-Info "  Peak:        ${peakMB} MB"
Write-Info "  Final:       ${finalMB} MB"
Write-Info "  Growth:      ${growthMB} MB over $([math]::Round($duration, 1))s"
Write-Info "  Growth rate: ${growthRate} MB/s"
Write-Info "  Samples:     $($memorySamples.Count)"

# Print sample table
Write-Host ""
Write-Host "  Events  | Memory (MB)" -ForegroundColor DarkGray
Write-Host "  --------|------------" -ForegroundColor DarkGray
foreach ($s in $memorySamples) {
    $bar = "#" * [math]::Min([math]::Max([int]($s.MemoryMB / 10), 1), 50)
    Write-Host ("  {0,6}  | {1,8:N1}  {2}" -f $s.Events, $s.MemoryMB, $bar) -ForegroundColor DarkGray
}
Write-Host ""

# ── Assertions ──────────────────────────────────────────────────────────────

if ($peakMB -le $MemoryLimitMB) {
    Write-Pass "Peak memory ${peakMB} MB within limit (${MemoryLimitMB} MB)"
} else {
    Write-Fail "Peak memory ${peakMB} MB EXCEEDS limit (${MemoryLimitMB} MB)"
}

# Growth rate check: the original leak was 300+ MB/s
# A healthy server should grow < 5 MB/s under scroll stress
if ($growthRate -lt 50) {
    Write-Pass "Growth rate ${growthRate} MB/s is acceptable"
} else {
    Write-Fail "Growth rate ${growthRate} MB/s suggests unbounded allocation"
}

# ── Check debug log ────────────────────────────────────────────────────────

Write-Test "Checking memory debug log..."
$logFile = "$PSMUX_DIR\memory_debug.log"
if (Test-Path $logFile) {
    $logLines = @(Get-Content $logFile -ErrorAction SilentlyContinue)
    Write-Info "  memory_debug.log: $($logLines.Count) lines"

    # Check for scroll rate entries
    $scrollRateLines = $logLines | Where-Object { $_ -match "scroll_rate" }
    if ($scrollRateLines.Count -gt 0) {
        Write-Pass "Debug log contains scroll rate data ($($scrollRateLines.Count) entries)"
        # Show last few
        $scrollRateLines | Select-Object -Last 3 | ForEach-Object {
            Write-Info "  $_"
        }
    } else {
        Write-Info "No scroll rate entries in debug log (instrumentation may not have triggered)"
    }

    # Check for frame push entries
    $framePushLines = $logLines | Where-Object { $_ -match "frame_push" }
    if ($framePushLines.Count -gt 0) {
        Write-Pass "Debug log contains frame push data ($($framePushLines.Count) entries)"
        $framePushLines | Select-Object -Last 3 | ForEach-Object {
            Write-Info "  $_"
        }
    }

    # Check for heartbeat entries
    $heartbeatLines = $logLines | Where-Object { $_ -match "heartbeat" }
    if ($heartbeatLines.Count -gt 0) {
        Write-Pass "Debug log has heartbeat snapshots ($($heartbeatLines.Count) entries)"
        $heartbeatLines | Select-Object -Last 1 | ForEach-Object {
            Write-Info "  $_"
        }
    }
} else {
    Write-Info "No memory_debug.log found (PSMUX_MEMORY_DEBUG may not have been active for server)"
}

# ── Cleanup ─────────────────────────────────────────────────────────────────

Write-Info "Cleaning up..."
& $PSMUX kill-session -t $SESSION 2>$null | Out-Null
Start-Sleep -Seconds 1
$env:PSMUX_MEMORY_DEBUG = $null

# ── Summary ─────────────────────────────────────────────────────────────────

Write-Host ""
Write-Host "═══════════════════════════════════════════════════" -ForegroundColor White
Write-Host "  Scroll Memory Test: $($script:TestsPassed) passed, $($script:TestsFailed) failed" -ForegroundColor $(if ($script:TestsFailed -gt 0) { "Red" } else { "Green" })
Write-Host "  Peak: ${peakMB} MB | Growth: ${growthMB} MB | Rate: ${growthRate} MB/s" -ForegroundColor White
Write-Host "═══════════════════════════════════════════════════" -ForegroundColor White

exit $script:TestsFailed
