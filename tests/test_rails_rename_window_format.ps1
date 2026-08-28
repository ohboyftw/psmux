# Rails bench: rename-window expands its argument as a format string.
#
# tmux treats the name as a format (cmd-rename-window.c → format_single_from_target).
# psmux expanded it on the bind-key/config/menu path but not on the CLI path,
# so `rename-window '#{pane_current_command}'` produced a window literally named
# "#{pane_current_command}" — the common path was the broken one (1c808c2).
. $PSScriptRoot/_harness.ps1

Write-Host "== rename-window format expansion ==" -ForegroundColor Cyan
$S = New-IsolatedSession "rename-fmt"
Start-Sleep -Milliseconds 400

Test-Case "rename-window expands a format sequence" {
    $null = psmux rename-window -t $S '#{window_index}-win' 2>&1
    Start-Sleep -Milliseconds 200
    $name = (psmux display-message -t $S -p '#{window_name}' 2>$null).Trim()
    $name -eq '0-win'
}

Test-Case "rename-window leaves a plain name untouched" {
    $null = psmux rename-window -t $S 'plain-name' 2>&1
    Start-Sleep -Milliseconds 200
    $name = (psmux display-message -t $S -p '#{window_name}' 2>$null).Trim()
    $name -eq 'plain-name'
}

Test-Case "a name with no format sequences survives special characters" {
    $null = psmux rename-window -t $S 'a-b_c.d' 2>&1
    Start-Sleep -Milliseconds 200
    $name = (psmux display-message -t $S -p '#{window_name}' 2>$null).Trim()
    $name -eq 'a-b_c.d'
}

Remove-PsmuxSession $S
Write-Summary
