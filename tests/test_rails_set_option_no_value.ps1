# Rails bench: set-option must not swallow a command that carries no value.
#
# The dispatch chain handled -u, then two-or-more positionals, then
# `len == 1 && -q`, and fell off the end for everything else — so `set -g @foo`
# set nothing, printed nothing and exited 0. On Windows that is a one-character
# accident: in PowerShell a bare `@name` is the splatting operator, so
# `set -g @pill $undefined` arrives as a single positional and every layer
# reports success while the value silently keeps its old contents.
#
# The empty-value form is the same defect from the other side: `set -g @foo ""`
# is how tmux CLEARS an option, and the tokenizer dropped the quoted empty token
# along with the whitespace, so the server saw a name with no value.
#
# `-g -v` rather than `-gv` throughout: bundled short flags are not merged by
# show-options, which is a separate known gap.
. $PSScriptRoot/_harness.ps1

Write-Host "== set-option with no value ==" -ForegroundColor Cyan
$S = New-IsolatedSession "setopt-novalue"
Start-Sleep -Milliseconds 600

function Get-Opt {
    param([string]$Name)
    return ((psmux show-options -t $S -g -v $Name 2>$null) -join '').Trim()
}

Test-Case "a set-option with a value still works (probe is valid)" {
    $null = psmux set-option -t $S -g '@rails_nv' 'first'
    ($LASTEXITCODE -eq 0) -and ((Get-Opt '@rails_nv') -eq 'first')
}

Test-Case "an option name with no value fails instead of vanishing" {
    $out = psmux set-option -t $S -g '@rails_nv' 2>&1
    $rc = $LASTEXITCODE
    $said = ($out -join ' ') -match 'empty value'
    if (-not ($rc -ne 0 -and $said)) {
        Write-Host "    rc=$rc out=[$($out -join ' ')]" -ForegroundColor DarkYellow
    }
    ($rc -ne 0) -and $said
}

Test-Case "the rejected command left the old value untouched" {
    (Get-Opt '@rails_nv') -eq 'first'
}

Test-Case "no positional at all reports too few arguments" {
    $out = psmux set-option -t $S -g 2>&1
    ($LASTEXITCODE -ne 0) -and (($out -join ' ') -match 'too few arguments')
}

# tmux 3.4 still fails `set -gq @foo`: -q scopes to unknown or ambiguous
# options, not to a missing value.
Test-Case "-q does not excuse a missing value" {
    $null = psmux set-option -t $S -gq '@rails_nv' 2>&1
    $LASTEXITCODE -ne 0
}

Test-Case "-u still accepts a single positional" {
    $null = psmux set-option -t $S -gu '@rails_nv' 2>&1
    $LASTEXITCODE -eq 0
}

Test-Case "an explicitly empty value clears the option" {
    $null = psmux set-option -t $S -g '@rails_empty' 'before'
    $null = psmux set-option -t $S -g '@rails_empty' ''
    ($LASTEXITCODE -eq 0) -and ((Get-Opt '@rails_empty') -eq '')
}

Remove-PsmuxSession $S
Write-Summary
