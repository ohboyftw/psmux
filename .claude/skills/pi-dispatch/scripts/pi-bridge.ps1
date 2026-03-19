<#
.SYNOPSIS
    Bridge Pi coding agent into Claude Code's TeammateTool inbox system.

.DESCRIPTION
    Runs a Pi coding agent with the given prompt, then writes the result
    to the TeammateTool leader inbox so the swarm coordinator knows the
    task completed (or failed).

.PARAMETER TeamName
    Name of the TeammateTool team (e.g., "psmux-swarm")

.PARAMETER AgentName
    Name this agent uses in inbox messages (e.g., "pi-worker-1")

.PARAMETER Prompt
    The task prompt to send to Pi

.PARAMETER WorkDir
    Working directory for Pi to run in (default: current directory)

.PARAMETER TaskId
    Optional TeammateTool task ID to mark as completed

.PARAMETER Provider
    Optional Pi provider override (e.g., "ollama", "openrouter")

.EXAMPLE
    .\pi-bridge.ps1 -TeamName psmux-swarm -AgentName pi-worker-1 `
        -Prompt "Add tests for parse_args" -WorkDir .worktrees/task-3 -TaskId 3
#>

param(
    [Parameter(Mandatory)]
    [string]$TeamName,

    [Parameter(Mandatory)]
    [string]$AgentName,

    [Parameter(Mandatory)]
    [string]$Prompt,

    [string]$WorkDir = ".",
    [string]$TaskId = "",
    [string]$Provider = ""
)

$ErrorActionPreference = "Stop"

# Resolve inbox path
$inboxDir = Join-Path $env:USERPROFILE ".claude" "teams" $TeamName "inboxes"
$inboxPath = Join-Path $inboxDir "team-lead.json"

# Ensure inbox directory exists
if (-not (Test-Path $inboxDir)) {
    New-Item -ItemType Directory -Path $inboxDir -Force | Out-Null
}

function Add-InboxMessage {
    param([string]$Path, [object]$Message)

    if (Test-Path $Path) {
        $content = Get-Content $Path -Raw -ErrorAction SilentlyContinue
        if ($content) {
            $inbox = $content | ConvertFrom-Json
            if ($inbox -isnot [System.Array]) {
                $inbox = @($inbox)
            }
        } else {
            $inbox = @()
        }
    } else {
        $inbox = @()
    }

    $inbox += $Message
    $inbox | ConvertTo-Json -Depth 10 | Set-Content $Path -Encoding UTF8
}

$timestamp = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")

# Set provider if specified
if ($Provider) {
    $env:PI_PROVIDER = $Provider
}

# Run Pi agent
Write-Host "[$AgentName] Starting Pi agent in $WorkDir" -ForegroundColor Cyan
Write-Host "[$AgentName] Prompt: $Prompt" -ForegroundColor Gray

Push-Location $WorkDir
try {
    $output = pi -p $Prompt 2>&1 | Out-String
    $exitCode = $LASTEXITCODE

    if ($exitCode -eq 0) {
        Write-Host "[$AgentName] Pi completed successfully" -ForegroundColor Green

        # Send result to leader inbox
        $message = [PSCustomObject]@{
            from      = $AgentName
            text      = $output.Trim()
            timestamp = $timestamp
            read      = $false
        }
        Add-InboxMessage -Path $inboxPath -Message $message

        # Send structured completion if task ID provided
        if ($TaskId) {
            $completion = [PSCustomObject]@{
                type        = "task_completed"
                from        = $AgentName
                taskId      = $TaskId
                taskSubject = "Completed by Pi agent"
                timestamp   = $timestamp
            }
            Add-InboxMessage -Path $inboxPath -Message $completion
        }
    } else {
        Write-Host "[$AgentName] Pi failed with exit code $exitCode" -ForegroundColor Red

        $message = [PSCustomObject]@{
            from      = $AgentName
            text      = "FAILED (exit $exitCode): $($output.Trim())"
            timestamp = $timestamp
            read      = $false
        }
        Add-InboxMessage -Path $inboxPath -Message $message
    }
} catch {
    Write-Host "[$AgentName] Exception: $($_.Exception.Message)" -ForegroundColor Red

    $message = [PSCustomObject]@{
        from      = $AgentName
        text      = "EXCEPTION: $($_.Exception.Message)"
        timestamp = $timestamp
        read      = $false
    }
    Add-InboxMessage -Path $inboxPath -Message $message
} finally {
    Pop-Location
}

Write-Host "[$AgentName] Result written to inbox: $inboxPath" -ForegroundColor Cyan
