# Rails bench: set-option -o (only-if-unset) via user_set_options HashSet
. $PSScriptRoot/_harness.ps1

Write-Host "== set-option -o ==" -ForegroundColor Cyan
$S = New-IsolatedSession "set-opt-o"

Test-Case "set -o on a fresh key applies the value" {
    psmux set-option -t $S -g -o status-left red 2>&1 | Out-Null
    $v = (psmux show-options -t $S -g -v status-left 2>$null).Trim()
    $v -eq 'red'
}

Test-Case "set -o on an already-set key is a no-op (value stays)" {
    psmux set-option -t $S -g -o status-left blue 2>&1 | Out-Null
    $v = (psmux show-options -t $S -g -v status-left 2>$null).Trim()
    $v -eq 'red'   # unchanged from first Test-Case
}

Test-Case "set without -o overrides (baseline sanity)" {
    psmux set-option -t $S -g status-left yellow 2>&1 | Out-Null
    $v = (psmux show-options -t $S -g -v status-left 2>$null).Trim()
    $v -eq 'yellow'
}

Test-Case "set -u removes the key from user_set_options; subsequent -o can set again" {
    psmux set-option -t $S -g -u status-left 2>&1 | Out-Null
    psmux set-option -t $S -g -o status-left green 2>&1 | Out-Null
    $v = (psmux show-options -t $S -g -v status-left 2>$null).Trim()
    $v -eq 'green'
}

Remove-PsmuxSession $S
Write-Summary
