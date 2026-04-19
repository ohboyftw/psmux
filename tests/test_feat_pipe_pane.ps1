# Feature bench: pipe-pane
. $PSScriptRoot/_harness.ps1

Write-Host "== pipe-pane ==" -ForegroundColor Cyan
$S = New-IsolatedSession "pipe-pane"
Start-Sleep -Milliseconds 500
$pane = (psmux display-message -t $S -p '#{pane_id}').Trim()

$log = Join-Path $env:TEMP "pipe-$((Get-Date).Ticks).log"

Test-Case "pipe-pane -o 'cat > $log' accepts the redirect command" {
    psmux pipe-pane -t $pane -o "cat > $log" 2>&1 | Out-Null
    $LASTEXITCODE -eq 0
}

Test-Case "text sent to the pane appears in the pipe log within 3s" {
    $marker = "__PIPE_$((Get-Date).Ticks)__"
    psmux send-keys -t $pane "Write-Host '$marker'" Enter | Out-Null
    Start-Sleep -Milliseconds 1500
    $found = $false
    for ($i = 0; $i -lt 6; $i++) {
        if (Test-Path $log) {
            $content = Get-Content $log -Raw -ErrorAction SilentlyContinue
            if ($content -and $content -match [regex]::Escape($marker)) { $found = $true; break }
        }
        Start-Sleep -Milliseconds 500
    }
    $found
}

Test-Case "pipe-pane with no args toggles off (no error)" {
    psmux pipe-pane -t $pane 2>&1 | Out-Null
    $LASTEXITCODE -eq 0
}

Remove-Item $log -Force -ErrorAction SilentlyContinue
Remove-PsmuxSession $S
Write-Summary
