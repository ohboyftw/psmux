<#
.SYNOPSIS
    Dispatch a task to a Pi coding agent in a psmux pane and return the result.

.DESCRIPTION
    Spawns a shell in a new psmux pane, sends Pi a task via send-keys (so Pi gets
    a real TTY and can use its tools), waits for completion, captures the pane
    output, and returns it.

.PARAMETER Prompt
    The task prompt to send to Pi.

.PARAMETER WorkDir
    Working directory for Pi (defaults to current directory).

.PARAMETER OutFile
    File to write the captured output to (defaults to temp file).

.PARAMETER Timeout
    Max seconds to wait for Pi to finish (default: 300 = 5 minutes).

.PARAMETER Quiet
    Suppress progress messages.

.EXAMPLE
    .\pi-dispatch.ps1 -Prompt "analyze src/server/mod.rs for race conditions"
    .\pi-dispatch.ps1 -Prompt "fix clippy warning in pane.rs" -WorkDir D:\Home\psmux
    .\pi-dispatch.ps1 -Prompt "write tests for parse_target" -Timeout 600
#>
param(
    [Parameter(Mandatory, Position = 0)]
    [string]$Prompt,

    [string]$WorkDir = (Get-Location).Path,

    [string]$OutFile = "",

    [int]$Timeout = 300,

    [switch]$Quiet
)

$ErrorActionPreference = "Stop"

# Generate temp output file if not specified
if (-not $OutFile) {
    $OutFile = Join-Path $env:TEMP "pi-dispatch-$(Get-Random).md"
}

# Clean up any previous run
Remove-Item $OutFile -ErrorAction SilentlyContinue

# Ensure a psmux session exists
$sessions = psmux list-sessions 2>&1
if (-not $sessions -or $LASTEXITCODE -ne 0) {
    if (-not $Quiet) { Write-Host "[pi-dispatch] No psmux session found, creating one..." -ForegroundColor Cyan }
    psmux new-session -d -s pi-workers 2>&1 | Out-Null
}

# Helper: poll #{pane_ready} instead of blind Start-Sleep to avoid send-keys truncation
function Wait-PaneReady {
    param([string]$Target, [int]$TimeoutSeconds = 25)
    $start = Get-Date
    while ($true) {
        $ready = (psmux display-message -t $Target -p "#{pane_ready}") 2>$null
        if ($ready -eq "1") { return $true }
        if (((Get-Date) - $start).TotalSeconds -gt $TimeoutSeconds) { return $false }
        Start-Sleep -Milliseconds 200
    }
}

# Spawn a new pane with a shell (detached, so we don't steal focus)
if (-not $Quiet) { Write-Host "[pi-dispatch] Spawning Pi agent..." -ForegroundColor Cyan }

$paneId = (psmux split-window -d -h -P -F "#{pane_id}") 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Error "Failed to spawn psmux pane: $paneId"
    exit 1
}
$paneId = $paneId.Trim()

# Wait for shell to be ready (poll instead of blind sleep)
if (-not (Wait-PaneReady $paneId)) { Write-Warning "Pane $paneId not ready after timeout" }

# Escape the prompt for shell safety
$safePrompt = $Prompt -replace "'", "''"
$safeWorkDir = $WorkDir -replace "\\", "/"

# Send commands via send-keys — Pi runs fully interactive with TTY
psmux send-keys -t $paneId "cd '$safeWorkDir'" Enter
if (-not (Wait-PaneReady $paneId 10)) { Write-Warning "Pane $paneId not ready after cd" }
# Run Pi in non-interactive mode (-p). No redirect — capture-pane gets the output.
# Touch a marker file when Pi finishes so we know it's done.
$safeMarker = ($OutFile + ".done") -replace "\\", "/"
psmux send-keys -t $paneId "pi -p '$safePrompt'; echo __PI_DONE__ > '$safeMarker'" Enter

if (-not $Quiet) { Write-Host "[pi-dispatch] Pi running in pane $paneId" -ForegroundColor Cyan }

# Poll for completion by checking if Pi process is still running
# We detect completion when the shell prompt returns (Pi has exited)
$startTime = Get-Date
$elapsed = 0
$lastLineCount = 0
$stableCount = 0

while ($elapsed -lt $Timeout) {
    Start-Sleep -Seconds 3
    $elapsed = ((Get-Date) - $startTime).TotalSeconds

    # Check if done marker exists (Pi wrote it after finishing)
    if (Test-Path "$OutFile.done") {
        if (-not $Quiet) { Write-Host "[pi-dispatch] Pi finished (marker detected)" -ForegroundColor Cyan }
        Start-Sleep -Seconds 1
        break
    }

    # Check if the pane still exists
    $paneCheck = psmux list-panes -F "#{pane_id}" 2>&1
    if ($paneCheck -notmatch [regex]::Escape($paneId)) {
        if (-not $Quiet) { Write-Host "[pi-dispatch] Pane $paneId exited" -ForegroundColor Yellow }
        Start-Sleep -Seconds 1
        break
    }

    # Capture current pane content and check if Pi is done
    # Pi is done when the shell prompt appears after Pi's output
    $captured = psmux capture-pane -t $paneId -p 2>&1
    $lines = ($captured -split "`n").Count

    # Check for common shell prompt patterns indicating Pi finished
    $lastLines = ($captured -split "`n" | Select-Object -Last 3) -join "`n"
    if ($lastLines -match '(PS [A-Z]:\\|>\s*$|\$\s*$|❯)' -and $elapsed -gt 5) {
        # Shell prompt is back — Pi has finished
        if (-not $Quiet) { Write-Host "[pi-dispatch] Pi finished (prompt detected)" -ForegroundColor Cyan }
        break
    }

    # Also detect stability — if output hasn't changed for 6 seconds after initial output
    if ($lines -eq $lastLineCount -and $lines -gt 5) {
        $stableCount++
        if ($stableCount -ge 2) {
            if (-not $Quiet) { Write-Host "[pi-dispatch] Pi finished (output stable)" -ForegroundColor Cyan }
            break
        }
    } else {
        $stableCount = 0
    }
    $lastLineCount = $lines

    if (-not $Quiet -and ($elapsed % 15 -lt 3)) {
        Write-Host "[pi-dispatch] Waiting... ($([math]::Floor($elapsed))s / ${Timeout}s, $lines lines)" -ForegroundColor DarkGray
    }
}

# Check for timeout
if ($elapsed -ge $Timeout) {
    Write-Host "[pi-dispatch] TIMEOUT after ${Timeout}s — killing pane $paneId" -ForegroundColor Red
    psmux kill-pane -t $paneId 2>&1 | Out-Null
    exit 2
}

# Capture the final pane output (use -S - to get full scrollback)
$rawOutput = psmux capture-pane -t $paneId -p -S - 2>&1

# Kill the pane (cleanup)
psmux kill-pane -t $paneId 2>&1 | Out-Null

# Clean up the output: strip shell prompts, cd commands, pi invocation, and assertion noise
$lines = $rawOutput -split "`n"
$cleaned = @()
$piStarted = $false
foreach ($line in $lines) {
    # Skip empty/whitespace lines at the start
    if (-not $piStarted) {
        # Detect when Pi's actual output begins (after the pi -p command line)
        if ($line -match "^pi -p " -or $line -match "pi -p '") {
            $piStarted = $true
            continue
        }
        continue
    }
    # Skip known noise lines
    if ($line -match '^\s*$' -and $cleaned.Count -eq 0) { continue }
    if ($line -match '^Loaded \d+ API key') { continue }
    if ($line -match '^PS [A-Z]:\\') { continue }
    if ($line -match '^Assertion failed:.*UV_HANDLE_CLOSING') { continue }
    if ($line -match '^\s*$' -and $cleaned.Count -gt 0 -and $cleaned[-1] -match '^\s*$') { continue }
    $cleaned += $line
}

# Trim trailing empty lines
while ($cleaned.Count -gt 0 -and $cleaned[-1] -match '^\s*$') {
    $cleaned = $cleaned[0..($cleaned.Count - 2)]
}

$result = $cleaned -join "`n"

# Write to file
if ($result) {
    $result | Set-Content $OutFile -Encoding UTF8
}

if (-not $Quiet) {
    $size = if (Test-Path $OutFile) { (Get-Item $OutFile).Length } else { 0 }
    Write-Host "[pi-dispatch] Done. Output ($size bytes):" -ForegroundColor Green
    Write-Host "---" -ForegroundColor DarkGray
}
Write-Output $result
