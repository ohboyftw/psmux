# Rails bench: capture-pane negative -S/-E clamps to row 0 (P1.8)
. $PSScriptRoot/_harness.ps1

Write-Host "== capture-pane neg clamp ==" -ForegroundColor Cyan
$S = New-IsolatedSession "capture-clamp"
Start-Sleep -Milliseconds 300
$pane = (psmux display-message -t $S -p '#{pane_id}').Trim()

Test-Case "capture-pane -S -5 -E -1 -p returns at most one row" {
    $out = psmux capture-pane -t $pane -S -5 -E -1 -p 2>$null
    $lines = ($out -split "`n") | Where-Object { $_ -ne '' }
    $lines.Count -le 1
}

Test-Case "capture-pane -S -100 -p does not error (clamps, doesn't crash)" {
    $null = psmux capture-pane -t $pane -S -100 -p 2>$null
    $LASTEXITCODE -eq 0
}

Test-Case "capture-pane with default range returns full visible pane" {
    $out = psmux capture-pane -t $pane -p 2>$null
    $LASTEXITCODE -eq 0 -and $out.Length -gt 0
}

Remove-PsmuxSession $S
Write-Summary
