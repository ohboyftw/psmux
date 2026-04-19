# Rails bench: source-file reload resets defaults_suppressed (P1.13)
. $PSScriptRoot/_harness.ps1

Write-Host "== source-file reload / defaults_suppressed ==" -ForegroundColor Cyan
$S = New-IsolatedSession "source-reload"

$conf1 = Join-Path $env:TEMP "psmux-test-conf1-$((Get-Date).Ticks).conf"
$conf2 = Join-Path $env:TEMP "psmux-test-conf2-$((Get-Date).Ticks).conf"
Set-Content -Path $conf1 -Value "unbind-key -a"
Set-Content -Path $conf2 -Value "# no unbind"

Test-Case "source-file with unbind-key -a suppresses defaults (few keys)" {
    psmux source-file -t $S $conf1 2>&1 | Out-Null
    Start-Sleep -Milliseconds 300
    $count = (psmux list-keys -t $S 2>$null | Measure-Object -Line).Lines
    $count -lt 5   # effectively no defaults
}

Test-Case "reloading a plain config restores default bindings" {
    psmux source-file -t $S $conf2 2>&1 | Out-Null
    Start-Sleep -Milliseconds 300
    $count = (psmux list-keys -t $S 2>$null | Measure-Object -Line).Lines
    $count -gt 20   # defaults have been re-registered
}

Remove-Item $conf1, $conf2 -Force -ErrorAction SilentlyContinue
Remove-PsmuxSession $S
Write-Summary
