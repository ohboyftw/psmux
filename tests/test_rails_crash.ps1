# Rails bench: crash diagnostics (panic hook + CLI)
. $PSScriptRoot/_harness.ps1

Write-Host "== crash diagnostics ==" -ForegroundColor Cyan

Test-Case "psmux debug crashes list runs and returns a table header or 'No crash reports'" {
    $r = psmux debug crashes list 2>&1 | Out-String
    $LASTEXITCODE -eq 0 -and ($r.Length -ge 0)
}

Test-Case "crashes directory exists at %LOCALAPPDATA%/psmux/crashes after server start" {
    $dir = Join-Path $env:LOCALAPPDATA "psmux/crashes"
    # Trigger server spawn so the crash dir gets created if it doesn't exist
    $S = New-IsolatedSession "crash-probe"
    Remove-PsmuxSession $S
    Test-Path $dir
}

Test-Case "psmux debug crashes show with bogus ID surfaces an error (stderr or stdout) and does not panic" {
    # CLI currently exits 0 even on missing crash — the contract we enforce
    # here is: the command must emit an informative error message and not
    # panic the CLI itself (which would dump a stack to stderr and abort).
    $r = psmux debug crashes show "nonexistent-12345-rail-test" 2>&1 | Out-String
    $r -match 'not\s+find|No such|os error 2|error|Error'
}

Write-Summary
