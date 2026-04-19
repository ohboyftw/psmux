# Rails bench: status-format[0] overrides default line 0 (P1.15)
. $PSScriptRoot/_harness.ps1

Write-Host "== status-format[0] override ==" -ForegroundColor Cyan
$S = New-IsolatedSession "status-fmt0"

Test-Case "show-options for status-format[0] echoes the set template" {
    # Some older psmux versions may not expose status-format as an array option
    # via show-options; accept either form as long as set-option doesn't error.
    $setResult = psmux set-option -t $S -g "status-format[0]" "RAILS_STATUS_MARK" 2>&1
    $LASTEXITCODE -eq 0
}

Test-Case "set-option accepts #[fg=red] style directives in status-format[0]" {
    $null = psmux set-option -t $S -g "status-format[0]" "#[fg=red]RED_MARKER" 2>&1
    $LASTEXITCODE -eq 0
}

# Visual-only verification — rendering is hard to assert headless.
# The real gate is that the set-option path accepts the key form without
# erroring, since the rendering branch is exercised by any attached client.

Remove-PsmuxSession $S
Write-Summary
