# Rails bench: set-option -o guard semantics
# Contract:
#   1. Plain set-option sets value.
#   2. set-option -o is a no-op when the option is already set (guard fires).
#   3. set-option -u clears the option from the "user-set" registry.
#   4. set-option -o on a cleared option sets it (guard no longer blocks).
#
# Uses @rails-guard-probe — an arbitrary user key (starts with @) that is
# safe to clobber and is not used by any other part of psmux.
. $PSScriptRoot/_harness.ps1

Write-Host "== set-option -o guard ==" -ForegroundColor Cyan
$S = New-IsolatedSession "setopt-guard"
Start-Sleep -Milliseconds 300

function Get-OptValue {
    (psmux show-options -t $S -g -v "@rails-guard-probe" 2>$null).Trim()
}

Test-Case "plain set-option -g stores value1" {
    psmux set-option -t $S -g "@rails-guard-probe" "value1" 2>&1 | Out-Null
    Start-Sleep -Milliseconds 200
    (Get-OptValue) -eq "value1"
}

Test-Case "set-option -g -o is a no-op when option already set (guard fires)" {
    psmux set-option -t $S -g -o "@rails-guard-probe" "value2" 2>&1 | Out-Null
    Start-Sleep -Milliseconds 200
    # value1 must survive — guard prevented the write
    (Get-OptValue) -eq "value1"
}

Test-Case "set-option -g -u clears the option" {
    psmux set-option -t $S -g -u "@rails-guard-probe" 2>&1 | Out-Null
    Start-Sleep -Milliseconds 200
    # After unset, show-options returns empty
    (Get-OptValue) -eq ""
}

Test-Case "set-option -g -o succeeds after -u cleared the guard" {
    psmux set-option -t $S -g -o "@rails-guard-probe" "value3" 2>&1 | Out-Null
    Start-Sleep -Milliseconds 200
    (Get-OptValue) -eq "value3"
}

Remove-PsmuxSession $S
Write-Summary
