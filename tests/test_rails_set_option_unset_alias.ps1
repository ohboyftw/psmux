# Rails bench: -U unsets, and clustered flags are seen at all.
#
# The two halves of the tree disagreed about the same line. The config parser
# read flags with `contains`, so it saw `-gu`; the server matched whole tokens,
# so it did not. An unset asked for as `set -gu @x` was dropped in silence, and
# because `-U` matched nothing at all, `set -U @x XX` WROTE XX where the caller
# asked for an unset — both at rc 0.
#
# `-g -v` rather than `-gv` throughout: bundled short flags are not merged by
# show-options, which is a separate known gap. The config-file half of the same
# alias is pinned by unit tests (tests-rs/test_set_option_unset_alias.rs): on
# that path a missed flag writes an empty value, which is indistinguishable
# from a real unset through show-options, so a rails case there would pass
# either way.
. $PSScriptRoot/_harness.ps1

Write-Host "== set-option -U / clustered flags ==" -ForegroundColor Cyan
$S = New-IsolatedSession "setopt-unset"
Start-Sleep -Milliseconds 600

function Get-Opt {
    param([string]$Name)
    return ((psmux show-options -t $S -g -v $Name 2>$null) -join '').Trim()
}
function Set-Base {
    param([string]$Name)
    $null = psmux set-option -t $S -g $Name 'base'
}

Test-Case "-u unsets (probe is valid)" {
    Set-Base '@rails_unset'
    $null = psmux set-option -t $S -u '@rails_unset'
    ($LASTEXITCODE -eq 0) -and ((Get-Opt '@rails_unset') -eq '')
}

Test-Case "-gu unsets, not just a bare -u" {
    Set-Base '@rails_unset'
    $null = psmux set-option -t $S -gu '@rails_unset'
    $v = Get-Opt '@rails_unset'
    if ($v -ne '') { Write-Host "    value survived as [$v]" -ForegroundColor DarkYellow }
    $v -eq ''
}

Test-Case "-U is an unset alias of -u" {
    Set-Base '@rails_unset'
    $null = psmux set-option -t $S -U '@rails_unset'
    $v = Get-Opt '@rails_unset'
    if ($v -ne '') { Write-Host "    value survived as [$v]" -ForegroundColor DarkYellow }
    $v -eq ''
}

# The dangerous one: the flag matched nothing, so the command fell through to
# the plain set path and wrote the trailing token. tmux unsets and ignores it.
Test-Case "-U with a trailing value unsets rather than writing it" {
    Set-Base '@rails_unset'
    $null = psmux set-option -t $S -U '@rails_unset' 'XX'
    $v = Get-Opt '@rails_unset'
    if ($v -ne '') { Write-Host "    wrote [$v] instead of unsetting" -ForegroundColor DarkYellow }
    $v -eq ''
}

Test-Case "a clustered -ga still appends" {
    $null = psmux set-option -t $S -g '@rails_append' 'one'
    $null = psmux set-option -t $S -ga '@rails_append' 'two'
    (Get-Opt '@rails_append') -match 'onetwo|one two'
}

Test-Case "an ordinary set still sets" {
    $null = psmux set-option -t $S -g '@rails_plain' 'kept'
    (Get-Opt '@rails_plain') -eq 'kept'
}

Remove-PsmuxSession $S
Write-Summary
