# Rails bench: set-option values survive the control wire intact.
#
# The client quoted a value only when it contained a SPACE, and escaped `"` but
# not `\`. Three things got through:
#   - a value holding `"` toggled quoting on the server and mangled the line
#   - a value ending in `\` (every Windows directory path) left the quoted
#     string unterminated
#   - a value holding a NEWLINE is whitespace but not a space, so it reached the
#     wire raw, the server's read_line cut the command there, and the tail ran
#     as a separate command (#560)
. $PSScriptRoot/_harness.ps1

Write-Host "== set-option value quoting ==" -ForegroundColor Cyan
$S = New-IsolatedSession "opt-quote"
Start-Sleep -Milliseconds 400

function Set-And-Read {
    param([string]$Value)
    $null = psmux set-option -t $S -g '@quote-probe' $Value 2>&1
    Start-Sleep -Milliseconds 150
    # `-g -v`, not `-gv`: bundled short flags are not merged by show-options,
    # and `-gv` returns "name value" instead of the value alone.
    return (psmux show-options -t $S -g -v '@quote-probe' 2>$null) -join "`n"
}

Test-Case "a value with spaces round-trips" {
    (Set-And-Read 'hello there world').Trim() -eq 'hello there world'
}

Test-Case "a value containing a double quote round-trips" {
    (Set-And-Read 'say "hi" now').Trim() -eq 'say "hi" now'
}

Test-Case "a Windows path ending in a backslash round-trips" {
    (Set-And-Read 'C:\some dir\').Trim() -eq 'C:\some dir\'
}

Test-Case "a UNC path with a doubled backslash round-trips" {
    (Set-And-Read '\\server\share').Trim() -eq '\\server\share'
}

Test-Case "a newline in the value cannot run a second command" {
    # If the newline reaches the wire raw, the server dispatches the tail as its
    # own command. `kill-window` would then destroy this session's only window.
    $before = @(psmux list-windows -t $S 2>$null | Where-Object { $_ -ne '' }).Count
    $null = psmux set-option -t $S -g '@quote-probe' "safe`nkill-window" 2>&1
    Start-Sleep -Milliseconds 300
    $after = @(psmux list-windows -t $S 2>$null | Where-Object { $_ -ne '' }).Count
    if ($after -ne $before) {
        Write-Host "    window count went $before -> ${after}: the tail executed" -ForegroundColor DarkYellow
    }
    $after -eq $before
}

Remove-PsmuxSession $S
Write-Summary
