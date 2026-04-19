# Rails bench: show-options -g -v without option name + default-shell resolution (P1.10)
. $PSScriptRoot/_harness.ps1

Write-Host "== show-options -g -v ==" -ForegroundColor Cyan
$S = New-IsolatedSession "show-opts"

Test-Case "show-options -g -v <name> returns value-only, no name prefix" {
    psmux set-option -t $S -g status-left "[#S]" 2>&1 | Out-Null
    $v = (psmux show-options -t $S -g -v status-left 2>$null).Trim()
    $v -eq '[#S]'
}

Test-Case "show-options -g -v without name lists values only (no 'key value' pairs)" {
    $out = psmux show-options -t $S -g -v 2>$null
    $lines = ($out -split "`n") | Where-Object { $_.Trim() -ne '' }
    # Values-only: no line should start with 'status-bg ' (key+space+value form)
    $anyKeyPrefix = $lines | Where-Object { $_ -match '^status-\w+ ' }
    ($lines.Count -gt 0) -and (-not $anyKeyPrefix)
}

Test-Case "show-options -g -v default-shell resolves to a real path (not empty)" {
    $shell = (psmux show-options -t $S -g -v default-shell 2>$null).Trim()
    # Must be non-empty and look like a shell name/path
    ($shell.Length -gt 0) -and ($shell -match 'pwsh|powershell|cmd|sh|bash')
}

Remove-PsmuxSession $S
Write-Summary
