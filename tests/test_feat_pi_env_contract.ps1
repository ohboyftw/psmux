# Feature bench: Pi / CustomPaneBackend env contract
# PSMUX=1, PSMUX_SESSION, PSMUX_PANE_ID, and ~/.psmux/{session}.pipe discovery
. $PSScriptRoot/_harness.ps1

Write-Host "== Pi env contract ==" -ForegroundColor Cyan
$S = New-IsolatedSession "pi-env"
Start-Sleep -Milliseconds 500
$pane = (psmux display-message -t $S -p '#{pane_id}').Trim()

function Read-PaneEnv {
    param([string]$Target, [string]$VarName)
    $marker = "__ENV_$VarName_$((Get-Date).Ticks)__"
    psmux send-keys -t $Target "Write-Host `"$marker=`$([Environment]::GetEnvironmentVariable('$VarName'))`"" Enter | Out-Null
    $ok = Wait-ForOutput -Target $Target -Pattern $marker -TimeoutMs 5000
    if (-not $ok) { return $null }
    $text = psmux capture-pane -t $Target -p 2>$null
    $line = ($text -split "`n") | Where-Object { $_ -match [regex]::Escape($marker) } | Select-Object -First 1
    if ($line -match "$([regex]::Escape($marker))=(.*)$") { return $Matches[1].Trim() } else { return $null }
}

Test-Case "PSMUX=1 is exported to child processes" {
    (Read-PaneEnv -Target $pane -VarName 'PSMUX') -eq '1'
}

Test-Case "PSMUX_SESSION matches the real session name" {
    (Read-PaneEnv -Target $pane -VarName 'PSMUX_SESSION') -eq $S
}

Test-Case "PSMUX_PANE_ID starts with '%' and matches pane id" {
    $v = Read-PaneEnv -Target $pane -VarName 'PSMUX_PANE_ID'
    $v -eq $pane
}

Test-Case "pipe discovery file exists at ~/.psmux/{session}.pipe" {
    $pipeFile = Join-Path $env:USERPROFILE ".psmux/$S.pipe"
    Test-Path $pipeFile
}

Remove-PsmuxSession $S
Write-Summary
