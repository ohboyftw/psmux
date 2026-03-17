<#
.SYNOPSIS
    Launch Claude Code with agent teams support via psmux.

.DESCRIPTION
    Creates a psmux session and launches Claude Code inside it with $TMUX set,
    enabling the split-pane tmux backend for agent teams. Each teammate gets
    its own visible psmux pane.

    This is the Windows-native alternative to running Claude Code inside tmux
    on Linux/macOS.

.PARAMETER SessionName
    psmux session name (default: claude-teams).

.PARAMETER WorkDir
    Working directory for Claude Code (default: current directory).

.PARAMETER ClaudeArgs
    Additional arguments to pass to Claude Code.

.EXAMPLE
    .\Start-ClaudeTeams.ps1
    .\Start-ClaudeTeams.ps1 -SessionName myproject -WorkDir D:\Home\psmux
    .\Start-ClaudeTeams.ps1 -ClaudeArgs "--model opus"
#>
param(
    [string]$SessionName = "claude-teams",
    [string]$WorkDir = (Get-Location).Path,
    [string]$ClaudeArgs = ""
)

$ErrorActionPreference = "Stop"

# Check psmux is installed
$psmux = Get-Command psmux -ErrorAction SilentlyContinue
if (-not $psmux) {
    Write-Error "psmux not found. Install with: cargo install psmux"
    exit 1
}

# Check Claude Code is installed
$claude = Get-Command claude -ErrorAction SilentlyContinue
if (-not $claude) {
    Write-Error "Claude Code not found. Install from: https://claude.ai/code"
    exit 1
}

Write-Host "Starting Claude Code with agent teams via psmux..." -ForegroundColor Cyan
Write-Host "  Session: $SessionName" -ForegroundColor DarkGray
Write-Host "  WorkDir: $WorkDir" -ForegroundColor DarkGray
Write-Host ""

# Kill existing session if any
psmux kill-session -t $SessionName 2>$null

# Create new psmux session with Claude Code as the initial command
# psmux sets $TMUX, $TMUX_PANE, and CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 automatically
$claudeCmd = "cd '$($WorkDir -replace "'","''")' && claude $ClaudeArgs"
psmux new-session -s $SessionName "$claudeCmd"
