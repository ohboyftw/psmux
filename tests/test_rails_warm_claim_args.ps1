# Rails bench: claiming a warm server honours -x/-y and the client's cwd.
#
# The warm pool is spawned at the pool's own geometry and in whatever directory
# the previous session ran from. The claim wire form carries the new name, and
# it used to carry ONLY that: the server read the first positional and passed
# None for the cwd, so `-x/-y` were ignored and the whole client-cwd branch in
# the ClaimSession handler was unreachable. A session therefore came out a
# different size and in a different directory depending on whether it happened
# to claim a warm server or cold-start — with no way to tell which had happened.
. $PSScriptRoot/_harness.ps1

Write-Host "== warm claim carries -x/-y and cwd ==" -ForegroundColor Cyan

function New-SizedSession {
    param([string]$Prefix, [int]$W, [int]$H, [switch]$NoWarm)
    $name = "$Prefix-" + [guid]::NewGuid().Guid.Substring(0, 8)
    $old = $env:PSMUX_NO_WARM
    if ($NoWarm) { $env:PSMUX_NO_WARM = '1' }
    $null = psmux new-session -d -s $name -x $W -y $H 2>&1
    $env:PSMUX_NO_WARM = $old
    Start-Sleep -Milliseconds 700
    return $name
}

function Get-SessionSize {
    param([string]$Name)
    $w = (psmux display-message -t $Name -p '#{window_width}' 2>$null).Trim()
    $h = (psmux display-message -t $Name -p '#{window_height}' 2>$null).Trim()
    return "${w}x${h}"
}

# Guarantee a warm server exists for the next claim: creating a session spawns a
# replacement for the one it consumed.
$primer = New-SizedSession -Prefix "warm-primer" -W 100 -H 30
Start-Sleep -Milliseconds 800

$warm = New-SizedSession -Prefix "warm-sized" -W 173 -H 47
$cold = New-SizedSession -Prefix "cold-sized" -W 173 -H 47 -NoWarm

Test-Case "a cold-started session honours -x/-y" {
    (Get-SessionSize $cold) -eq '173x47'
}

Test-Case "a warm-claimed session is the same size as a cold-started one" {
    $w = Get-SessionSize $warm
    $c = Get-SessionSize $cold
    if ($w -ne $c) { Write-Host "    warm=$w cold=$c" -ForegroundColor DarkYellow }
    $w -eq $c
}

Test-Case "the claimed session starts in the client's working directory" {
    # The handler injects `cd` into the active pane, so give the shell a moment.
    $expected = (Get-Location).Path
    $name = "warm-cwd-" + [guid]::NewGuid().Guid.Substring(0, 8)
    $null = psmux new-session -d -s $name 2>&1
    Start-Sleep -Milliseconds 1500
    $got = (psmux display-message -t $name -p '#{pane_current_path}' 2>$null).Trim()
    $ok = $got -and ($got.TrimEnd('\') -ieq $expected.TrimEnd('\'))
    if (-not $ok) { Write-Host "    expected=$expected got=$got" -ForegroundColor DarkYellow }
    Remove-PsmuxSession $name
    $ok
}

Remove-PsmuxSession $primer
Remove-PsmuxSession $warm
Remove-PsmuxSession $cold
Write-Summary
