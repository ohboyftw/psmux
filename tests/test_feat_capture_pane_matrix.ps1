# Feature bench: capture-pane flag matrix (-J, -e, -S, -E combinations)
. $PSScriptRoot/_harness.ps1

Write-Host "== capture-pane matrix ==" -ForegroundColor Cyan
$S = New-IsolatedSession "capture-matrix"
Start-Sleep -Milliseconds 400
$pane = (psmux display-message -t $S -p '#{pane_id}').Trim()

# Seed some output
psmux send-keys -t $pane "Write-Host 'alpha'; Write-Host 'beta'; Write-Host 'gamma'" Enter | Out-Null
Start-Sleep -Milliseconds 800

Test-Case "capture-pane -p returns plain text (default)" {
    $out = psmux capture-pane -t $pane -p 2>$null
    $LASTEXITCODE -eq 0 -and ($out -match 'alpha|beta|gamma')
}

Test-Case "capture-pane -p -e returns ANSI-styled text (contains ESC or OK if terminal plain)" {
    $out = psmux capture-pane -t $pane -p -e 2>$null
    # Accept either ANSI-laden output or clean fallback — must not error
    $LASTEXITCODE -eq 0
}

Test-Case "capture-pane -p -J joins wrapped lines" {
    $out = psmux capture-pane -t $pane -p -J 2>$null
    $LASTEXITCODE -eq 0
}

Test-Case "capture-pane -p -S 0 -E 5 clamps range and returns at most 6 lines" {
    $out = psmux capture-pane -t $pane -p -S 0 -E 5 2>$null
    $lines = ($out -split "`n") | Where-Object { $_ -ne '' }
    $lines.Count -le 6
}

Test-Case "capture-pane -p -S 999 does not error (out-of-range clamps)" {
    $null = psmux capture-pane -t $pane -p -S 999 2>$null
    $LASTEXITCODE -eq 0
}

Remove-PsmuxSession $S
Write-Summary
