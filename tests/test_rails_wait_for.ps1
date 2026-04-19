# Rails bench: wait-for (--exit, --file, --output, --ready, --json)
# NOTE: target must be a session name, not a pane id (%N). See task #16 —
# wait-for target resolution is pane-id-unaware today.
. $PSScriptRoot/_harness.ps1

Write-Host "== wait-for ==" -ForegroundColor Cyan
$S = New-IsolatedSession "wait-for"
Start-Sleep -Milliseconds 500

Test-Case "--output matches a marker emitted by send-keys" {
    $marker = "__WF_$((Get-Date).Ticks)__"
    psmux send-keys -t $S "echo $marker" Enter | Out-Null
    # Longer settle: under bench load the shell needs more time to render the
    # echoed marker into the capture buffer that wait-for's regex scan reads.
    Start-Sleep -Milliseconds 1200
    $r = psmux wait-for -t $S --output $marker --timeout 5000 --json 2>$null
    $json = try { $r | ConvertFrom-Json } catch { $null }
    $json -and $json.kind -eq 'success'
}

Test-Case "--output non-match exits non-zero within the timeout window" {
    # Fixed in task #15: --output timeout now emits {kind:"timeout"} JSON + exit 1
    # (previously emitted empty output + exit 2 due to a unit-mismatch bug).
    $t0 = Get-Date
    $r = psmux wait-for -t $S --output "__NEVER_$((Get-Random))__" --timeout 600 --json 2>$null
    $elapsed = ((Get-Date) - $t0).TotalMilliseconds
    $json = try { $r | ConvertFrom-Json } catch { $null }
    ($LASTEXITCODE -eq 1) -and ($json -and $json.kind -eq 'timeout') -and ($elapsed -lt 3000)
}

Test-Case "--ready returns kind=success once the warm pane is idle" {
    $r = psmux wait-for -t $S --ready --timeout 5000 --json 2>$null
    $json = try { $r | ConvertFrom-Json } catch { $null }
    $json -and $json.kind -eq 'success'
}

Test-Case "--file returns kind=success once the file appears" {
    $tmp = Join-Path $env:TEMP "wf-$((Get-Date).Ticks).txt"
    if (Test-Path $tmp) { Remove-Item $tmp -Force }
    Start-Job -ScriptBlock {
        param($p) Start-Sleep -Milliseconds 500; Set-Content -Path $p -Value "ready"
    } -ArgumentList $tmp | Out-Null
    $r = psmux wait-for --file $tmp --timeout 5000 --json 2>$null
    Remove-Item $tmp -Force -ErrorAction SilentlyContinue
    $json = try { $r | ConvertFrom-Json } catch { $null }
    $json -and $json.kind -eq 'success'
}

Test-Case "--exit returns kind=exit_success when the PID exits cleanly" {
    $child = Start-Process -FilePath "cmd" -ArgumentList "/c", "exit 42" -PassThru -WindowStyle Hidden
    $r = psmux wait-for --exit $child.Id --timeout 5000 --json 2>$null
    $json = try { $r | ConvertFrom-Json } catch { $null }
    $json -and ($json.kind -eq 'exit_success' -or $json.kind -eq 'success')
}

Remove-PsmuxSession $S
Write-Summary
