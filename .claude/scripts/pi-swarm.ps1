<#
.SYNOPSIS
    Spawn a swarm of Pi coding agents in parallel psmux panes.

.DESCRIPTION
    Takes a list of tasks (as a JSON file or inline array), spawns each in its own
    psmux pane (or window in -Headless mode), waits for all to complete, and returns
    collected results. Use -Headless for unlimited parallel agents (no pane size constraint).

.PARAMETER Tasks
    JSON array of task objects: [{"name": "task1", "prompt": "...", "workdir": "..."}]
    Or a path to a JSON file containing the array.

.PARAMETER Provider
    Pi provider override (e.g., "anthropic", "google", "openai", "ollama").

.PARAMETER Model
    Pi model override (e.g., "claude-sonnet", "gemini-2.0-flash").

.PARAMETER Timeout
    Max seconds to wait for ALL tasks (default: 600 = 10 minutes).

.PARAMETER Session
    psmux session name (default: pi-swarm).

.PARAMETER Quiet
    Suppress progress messages.

.EXAMPLE
    # Inline JSON
    .\pi-swarm.ps1 -Tasks '[{"name":"analyze","prompt":"analyze src/main.rs"},{"name":"tests","prompt":"list all test functions"}]'

    # From file
    .\pi-swarm.ps1 -Tasks tasks.json

    # With provider override
    .\pi-swarm.ps1 -Tasks tasks.json -Provider ollama -Model qwen3

    # Headless mode: unlimited agents (windows instead of panes)
    .\pi-swarm.ps1 -Tasks tasks.json -Headless
#>
param(
    [Parameter(Mandatory, Position = 0)]
    [string]$Tasks,

    [string]$Provider = "",

    [string]$Model = "",

    [int]$Timeout = 600,

    [string]$Session = "pi-swarm",

    [string]$WorkDir = (Get-Location).Path,

    [switch]$Quiet,

    [switch]$Headless
)

$ErrorActionPreference = "Stop"

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

# ── Parse tasks ──
if (Test-Path $Tasks) {
    $taskList = Get-Content $Tasks -Raw | ConvertFrom-Json
} else {
    $taskList = $Tasks | ConvertFrom-Json
}

if ($taskList.Count -eq 0) {
    Write-Error "No tasks provided."
    exit 1
}

if (-not $Quiet) {
    Write-Host "[pi-swarm] Launching $($taskList.Count) Pi agents..." -ForegroundColor Cyan
}

# ── Output directory ──
$outDir = Join-Path $env:TEMP "pi-swarm-$(Get-Random)"
New-Item -ItemType Directory -Path $outDir -Force | Out-Null

# ── Ensure psmux session exists ──
$sessions = psmux list-sessions 2>&1
if (-not $sessions -or $sessions -notmatch $Session) {
    psmux new-session -d -s $Session 2>&1 | Out-Null
    if (-not $Quiet) { Write-Host "[pi-swarm] Created session '$Session'" -ForegroundColor DarkGray }
}

# ── Build Pi command prefix ──
$piFlags = ""
if ($Provider) { $piFlags += " --provider $Provider" }
if ($Model) { $piFlags += " --model $Model" }

# ── Spawn each task in its own pane ──
$panes = @()
for ($i = 0; $i -lt $taskList.Count; $i++) {
    $task = $taskList[$i]
    $name = if ($task.name) { $task.name } else { "task-$i" }
    $prompt = $task.prompt -replace "'", "''"
    $taskWorkDir = if ($task.workdir) { $task.workdir -replace "\\", "/" } else { $WorkDir -replace "\\", "/" }
    $markerFile = (Join-Path $outDir "$name.done") -replace "\\", "/"
    $outFile = (Join-Path $outDir "$name.md") -replace "\\", "/"

    # Create pane (split) or window (headless) for each agent
    if ($Headless) {
        # Headless: each agent gets its own full-size window — no pane limit
        $paneId = (psmux new-window -d -t $Session -P -F "#{pane_id}") 2>&1
    } else {
        # Visible: split panes for monitoring (limited by terminal size)
        $paneId = (psmux split-window -d -h -t $Session -P -F "#{pane_id}") 2>&1
    }
    $paneId = $paneId.Trim()

    if (-not $Headless) {
        # Rebalance layout after each split (only needed for panes)
        psmux select-layout -t $Session tiled 2>&1 | Out-Null
    }

    # Wait for shell to be ready (poll instead of blind sleep)
    if (-not (Wait-PaneReady $paneId)) { Write-Warning "Pane $paneId ($name) not ready after timeout" }

    # Send commands
    psmux send-keys -t $paneId "cd '$taskWorkDir'" Enter
    if (-not (Wait-PaneReady $paneId 10)) { Write-Warning "Pane $paneId ($name) not ready after cd" }
    psmux send-keys -t $paneId "pi$piFlags -p '$prompt' > '$outFile' 2>&1; echo done > '$markerFile'; exit" Enter

    $panes += @{
        id     = $paneId
        name   = $name
        marker = (Join-Path $outDir "$name.done")
        output = (Join-Path $outDir "$name.md")
        done   = $false
    }

    if (-not $Quiet) {
        Write-Host "[pi-swarm]   [$name] → pane $paneId" -ForegroundColor DarkGray
    }
}

if (-not $Quiet) {
    Write-Host "[pi-swarm] All $($panes.Count) agents spawned. Waiting..." -ForegroundColor Cyan
}

# ── Poll for completion ──
$startTime = Get-Date
$elapsed = 0
$completedCount = 0

while ($elapsed -lt $Timeout -and $completedCount -lt $panes.Count) {
    Start-Sleep -Seconds 3
    $elapsed = ((Get-Date) - $startTime).TotalSeconds

    for ($i = 0; $i -lt $panes.Count; $i++) {
        if ($panes[$i].done) { continue }

        if (Test-Path $panes[$i].marker) {
            $panes[$i].done = $true
            $completedCount++
            $dur = [math]::Floor($elapsed)
            if (-not $Quiet) {
                Write-Host "[pi-swarm]   [$($panes[$i].name)] completed (${dur}s)" -ForegroundColor Green
            }
        }
    }

    if (-not $Quiet -and ($elapsed % 15 -lt 3) -and $completedCount -lt $panes.Count) {
        Write-Host "[pi-swarm] Progress: $completedCount/$($panes.Count) done ($([math]::Floor($elapsed))s)" -ForegroundColor DarkGray
    }
}

# ── Handle timeout ──
$timedOut = @()
foreach ($p in $panes) {
    if (-not $p.done) {
        $timedOut += $p.name
        if ($Headless) {
            psmux kill-window -t $p.id 2>&1 | Out-Null
        } else {
            psmux kill-pane -t $p.id 2>&1 | Out-Null
        }
    }
}

if ($timedOut.Count -gt 0) {
    Write-Host "[pi-swarm] TIMEOUT: $($timedOut -join ', ') did not finish" -ForegroundColor Red
}

# ── Collect and return results ──
if (-not $Quiet) {
    Write-Host "[pi-swarm] ═══ Results ═══" -ForegroundColor Cyan
}

$results = @{}
foreach ($p in $panes) {
    $content = ""
    if (Test-Path $p.output) {
        $content = (Get-Content $p.output -Raw -Encoding UTF8).Trim()
    }

    $results[$p.name] = @{
        name      = $p.name
        completed = $p.done
        output    = $content
    }

    if (-not $Quiet) {
        $status = if ($p.done) { "OK" } else { "TIMEOUT" }
        $color = if ($p.done) { "Green" } else { "Red" }
        Write-Host "─── [$($p.name)] ($status) ───" -ForegroundColor $color
        if ($content) {
            # Show first 20 lines
            $preview = ($content -split "`n" | Select-Object -First 20) -join "`n"
            Write-Host $preview
            $totalLines = ($content -split "`n").Count
            if ($totalLines -gt 20) {
                Write-Host "... ($($totalLines - 20) more lines in $($p.output))" -ForegroundColor DarkGray
            }
        } else {
            Write-Host "(no output)" -ForegroundColor Yellow
        }
        Write-Host ""
    }
}

# ── Summary ──
if (-not $Quiet) {
    $totalTime = [math]::Floor(((Get-Date) - $startTime).TotalSeconds)
    Write-Host "[pi-swarm] Done: $completedCount/$($panes.Count) completed in ${totalTime}s" -ForegroundColor Cyan
    Write-Host "[pi-swarm] Output files in: $outDir" -ForegroundColor DarkGray
}

# Return results as JSON for programmatic consumption
$results | ConvertTo-Json -Depth 3
