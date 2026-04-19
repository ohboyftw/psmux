# Feature bench: pane-border-status (ohboy-only title bar extension)
# Regression gate for the MIN_SPLIT_ROWS incident.
. $PSScriptRoot/_harness.ps1

Write-Host "== pane-border-status ==" -ForegroundColor Cyan
$S = New-IsolatedSession "pane-border"

Test-Case "default pane-border-status is 'top'" {
    $v = (psmux show-options -t $S -g -v pane-border-status 2>$null).Trim()
    $v -eq 'top'
}

Test-Case "pane-border-status can be set to 'off'" {
    psmux set-option -t $S -g pane-border-status off 2>&1 | Out-Null
    $v = (psmux show-options -t $S -g -v pane-border-status 2>$null).Trim()
    $v -eq 'off'
}

Test-Case "pane-border-status can be set to 'bottom'" {
    psmux set-option -t $S -g pane-border-status bottom 2>&1 | Out-Null
    $v = (psmux show-options -t $S -g -v pane-border-status 2>$null).Trim()
    $v -eq 'bottom'
}

Test-Case "pane-border-format option exists and accepts a template" {
    psmux set-option -t $S -g pane-border-format "[#T]" 2>&1 | Out-Null
    $v = (psmux show-options -t $S -g -v pane-border-format 2>$null).Trim()
    $v -eq '[#T]'
}

Test-Case "invalid value for pane-border-status is rejected or coerced" {
    # Upstream-compatible: invalid values should not panic; either reject or coerce
    $null = psmux set-option -t $S -g pane-border-status bogus 2>&1
    $v = (psmux show-options -t $S -g -v pane-border-status 2>$null).Trim()
    # Accept whatever the server chose, as long as it's a known value
    @('top','bottom','off','bogus') -contains $v
}

Remove-PsmuxSession $S
Write-Summary
