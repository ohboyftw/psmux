# Integration audit — Pi v0.70.6 + Claude Code v2.1.123 contracts
#
# CHECK script: validates that the env-var + pipe-discovery contracts the
# Pi adapter (PsmuxAdapter v0.9.14+) and Claude Code TeammateTool depend on
# are still upheld by ohboy-builds after the sync-2026-04-29-harrier API
# surface review.
#
# Companion doc: .claude/internal/api-audit-pi070-cc2123-2026-04-29.md
#
# Contracts verified (all derived from the audit doc):
#   1. PSMUX=1 exported to child processes
#   2. PSMUX_SESSION matches the real session name
#   3. PSMUX_PANE_ID starts with '%' and matches the pane id
#   4. CLAUDE_PANE_BACKEND_SOCKET is exported and points to a \\.\pipe\... path
#   5. PI_PANE_BACKEND_SOCKET is exported and equals CLAUDE_PANE_BACKEND_SOCKET
#   6. TMUX_PANE is exported (legacy tmux compat — Claude Code reads this)
#   7. CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 (gate for visible team panes)
#   8. ~/.psmux/{session}.pipe discovery file exists shortly after start
#   9. The named pipe is reachable (round-trip: open + close handle)
#  10. PowerShell-fallback contract (CC v2.1.120) — pi-dispatch.ps1 +
#      pi-swarm.ps1 + Start-ClaudeTeams.ps1 + pi-bridge.ps1 contain no
#      bash/git-bash invocations
#
# Idempotent. Cleans up its own session. Exit code = number of failures.

. $PSScriptRoot/_harness.ps1

Write-Host "== Pi v0.70.6 + CC v2.1.123 integration audit ==" -ForegroundColor Cyan

$S = New-IsolatedSession "pi-cc-audit"
Start-Sleep -Milliseconds 500
$pane = (psmux display-message -t $S -p '#{pane_id}').Trim()

function Read-PaneEnv {
    param([string]$Target, [string]$VarName)
    $marker = "__ENVAUD_${VarName}_$((Get-Date).Ticks)__"
    psmux send-keys -t $Target "Write-Host `"$marker=`$([Environment]::GetEnvironmentVariable('$VarName'))`"" Enter | Out-Null
    $ok = Wait-ForOutput -Target $Target -Pattern $marker -TimeoutMs 5000
    if (-not $ok) { return $null }
    $text = psmux capture-pane -t $Target -p 2>$null
    $line = ($text -split "`n") | Where-Object { $_ -match [regex]::Escape($marker) } | Select-Object -First 1
    if ($line -match "$([regex]::Escape($marker))=(.*)$") { return $Matches[1].Trim() } else { return $null }
}

# ── Env-var contract (per Known Ohboy-Only Extensions table — PSMUX_* row) ──

Test-Case "PSMUX=1 is exported to child processes" {
    (Read-PaneEnv -Target $pane -VarName 'PSMUX') -eq '1'
}

Test-Case "PSMUX_SESSION matches the real session name" {
    (Read-PaneEnv -Target $pane -VarName 'PSMUX_SESSION') -eq $S
}

Test-Case "PSMUX_PANE_ID equals the pane id and starts with '%'" {
    $v = Read-PaneEnv -Target $pane -VarName 'PSMUX_PANE_ID'
    ($v -eq $pane) -and ($v -like '%*')
}

Test-Case "CLAUDE_PANE_BACKEND_SOCKET points to a \\.\pipe\\ path" {
    $v = Read-PaneEnv -Target $pane -VarName 'CLAUDE_PANE_BACKEND_SOCKET'
    $v -like '\\.\pipe\psmux-claude-backend-*'
}

Test-Case "PI_PANE_BACKEND_SOCKET equals CLAUDE_PANE_BACKEND_SOCKET" {
    $cc = Read-PaneEnv -Target $pane -VarName 'CLAUDE_PANE_BACKEND_SOCKET'
    $pi = Read-PaneEnv -Target $pane -VarName 'PI_PANE_BACKEND_SOCKET'
    ($cc -ne $null) -and ($cc -eq $pi)
}

Test-Case "TMUX_PANE is exported (CC reads this for tmux-compat detection)" {
    $v = Read-PaneEnv -Target $pane -VarName 'TMUX_PANE'
    ($v -ne $null) -and ($v -eq $pane)
}

Test-Case "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 (visible-pane gate)" {
    (Read-PaneEnv -Target $pane -VarName 'CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS') -eq '1'
}

# ── Pipe-discovery contract (per memory: "early discovery file write at session start") ──

Test-Case "~/.psmux/{session}.pipe discovery file exists shortly after session start" {
    $pipeFile = Join-Path $env:USERPROFILE ".psmux/$S.pipe"
    Test-Path $pipeFile
}

Test-Case "Discovery file content matches CLAUDE_PANE_BACKEND_SOCKET env var" {
    $pipeFile = Join-Path $env:USERPROFILE ".psmux/$S.pipe"
    if (-not (Test-Path $pipeFile)) { return $false }
    $disk = (Get-Content $pipeFile -Raw -ErrorAction SilentlyContinue).Trim()
    $env  = Read-PaneEnv -Target $pane -VarName 'CLAUDE_PANE_BACKEND_SOCKET'
    ($disk -ne '') -and ($disk -eq $env)
}

# ── Named-pipe reachability (round-trip handle open) ──

Test-Case "Named pipe is reachable (open handle round-trip)" {
    $pipeName = "psmux-claude-backend-$S"
    $client = New-Object System.IO.Pipes.NamedPipeClientStream(
        '.', $pipeName,
        [System.IO.Pipes.PipeDirection]::InOut,
        [System.IO.Pipes.PipeOptions]::None
    )
    try {
        $client.Connect(2000)  # 2s — server should be listening already
        $reachable = $client.IsConnected
        $client.Dispose()
        return $reachable
    } catch {
        if ($client) { $client.Dispose() }
        return $false
    }
}

# ── PowerShell-fallback contract (CC v2.1.120 — Git Bash no longer required) ──
#
# These four scripts must be runnable on a Git-Bash-less machine.  Verified by
# static grep — bash/git-bash invocations would defeat the whole point.

$repoRoot = Split-Path -Parent $PSScriptRoot
$scriptsToCheck = @(
    Join-Path $repoRoot '.claude/scripts/pi-dispatch.ps1'
    Join-Path $repoRoot '.claude/scripts/pi-swarm.ps1'
    Join-Path $repoRoot '.claude/skills/pi-dispatch/scripts/pi-bridge.ps1'
    Join-Path $repoRoot 'scripts/Start-ClaudeTeams.ps1'
)

foreach ($script in $scriptsToCheck) {
    $name = Split-Path -Leaf $script
    Test-Case "PowerShell-fallback: '$name' has no bash/git-bash invocations" {
        if (-not (Test-Path $script)) { return $false }
        $content = Get-Content $script -Raw -Encoding UTF8
        # Allowlist: comments and the word "bash" inside a docstring / comment
        # are fine; we look for actual invocations.
        $lines = $content -split "`n"
        foreach ($line in $lines) {
            $stripped = ($line -replace '#.*$', '').Trim()
            if ($stripped -match '(\bbash\.exe\b|\bsh\.exe\b|/bin/bash|/bin/sh|\bgit-bash\b)') {
                return $false
            }
        }
        return $true
    }
}

Remove-PsmuxSession $S
Write-Summary
