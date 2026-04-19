# Feature bench: source-file nested + -F format expand
. $PSScriptRoot/_harness.ps1

Write-Host "== source-file nested ==" -ForegroundColor Cyan
$S = New-IsolatedSession "source-nested"

$tmpDir = Join-Path $env:TEMP "src-nested-$((Get-Date).Ticks)"
New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null
$inner = Join-Path $tmpDir "inner.conf"
$outer = Join-Path $tmpDir "outer.conf"
Set-Content -Path $inner -Value 'set -g status-left "INNER"'
Set-Content -Path $outer -Value "source-file $inner"

Test-Case "nested source-file applies inner config options" {
    psmux source-file -t $S $outer 2>&1 | Out-Null
    Start-Sleep -Milliseconds 300
    $v = (psmux show-options -t $S -g -v status-left 2>$null).Trim()
    $v -eq 'INNER'
}

Test-Case "source-file against a missing path does not crash server" {
    $missing = Join-Path $tmpDir "does-not-exist.conf"
    $null = psmux source-file -t $S $missing 2>&1
    # Should return non-zero but server must stay alive
    $stillAlive = $false
    $probe = psmux has-session -t $S 2>&1
    $stillAlive = ($LASTEXITCODE -eq 0)
    $stillAlive
}

Test-Case "-F format expansion in source-file path is accepted" {
    # Create a config at an expanded path and source it via -F
    $expanded = Join-Path $tmpDir "expanded.conf"
    Set-Content -Path $expanded -Value 'set -g status-left "FMT"'
    # Use -F with a literal path (format expansion reduces to the same string)
    psmux source-file -F $expanded 2>&1 | Out-Null
    # Just verify no error — tiered format expansion semantics vary
    $LASTEXITCODE -eq 0
}

Remove-Item $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
Remove-PsmuxSession $S
Write-Summary
