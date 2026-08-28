# Rails bench: capture-pane -N preserves trailing whitespace.
#
# Full-screen TUIs paint their background to end-of-line with styled spaces.
# The default capture trims after the last non-whitespace cell, dropping those
# spaces and the SGR run that colours them. -N keeps the full row width.
#
# The two halves are atomic: server-side -N with no client forwarding is
# reachable only over control mode or a raw socket, and forwarding a flag the
# server ignores changes nothing.
. $PSScriptRoot/_harness.ps1

Write-Host "== capture-pane -N ==" -ForegroundColor Cyan
$S = New-IsolatedSession "capture-n"
Start-Sleep -Milliseconds 500

# Known geometry so "full width" is a number we can assert against.
$null = psmux new-session -d -s "${S}-sized" -x 100 -y 24 2>&1
Start-Sleep -Milliseconds 700
$T = "${S}-sized"
$width = [int](psmux display-message -t $T -p '#{window_width}' 2>$null).Trim()

Test-Case "the fixture reports a usable width" {
    $width -gt 0
}

# psmux emits one array element per line already; -split on the array collapses
# it to a single string and then iterates CHARACTERS, so normalise explicitly.
function Get-CaptureLines {
    param([string[]]$Out)
    return @($Out | ForEach-Object { [string]$_ } | Where-Object { $_ -ne '' })
}

# Blank rows are the precise probe. A row carrying prompt text can be LONGER in
# UTF-16 units than the pane is wide — a glyph can cost two units and one column
# — so asserting `length == width` on every row measures the prompt font, not the
# flag. A blank row contains only spaces, where units and columns agree exactly.

Test-Case "plain capture drops blank rows to nothing" {
    $lines = Get-CaptureLines (psmux capture-pane -t $T -p 2>$null)
    # Get-CaptureLines strips empties, so a trimmed blank row disappears entirely:
    # every surviving line must carry content.
    @($lines | Where-Object { $_.Trim() -eq '' }).Count -eq 0
}

Test-Case "capture-pane -N pads blank rows to the full pane width" {
    $raw = @(psmux capture-pane -t $T -p -N 2>$null | ForEach-Object { [string]$_ })
    $blank = @($raw | Where-Object { $_.Trim() -eq '' })
    $wrong = @($blank | Where-Object { $_.Length -ne $width })
    if ($blank.Count -eq 0) {
        Write-Host "    no blank rows captured; -N is not preserving them" -ForegroundColor DarkYellow
    } elseif ($wrong.Count -gt 0) {
        Write-Host "    width=$width first blank row len=$($wrong[0].Length)" -ForegroundColor DarkYellow
    }
    ($blank.Count -gt 0) -and ($wrong.Count -eq 0)
}

Test-Case "capture-pane -N keeps rows the plain capture would have trimmed" {
    $plain = @(psmux capture-pane -t $T -p 2>$null | ForEach-Object { [string]$_ })
    $kept = @(psmux capture-pane -t $T -p -N 2>$null | ForEach-Object { [string]$_ })
    $plainTotal = ($plain | Measure-Object -Property Length -Sum).Sum
    $keptTotal = ($kept | Measure-Object -Property Length -Sum).Sum
    if ($keptTotal -le $plainTotal) {
        Write-Host "    plain=$plainTotal preserved=$keptTotal" -ForegroundColor DarkYellow
    }
    $keptTotal -gt $plainTotal
}

Test-Case "capture-pane -N exits 0" {
    $null = psmux capture-pane -t $T -p -N 2>&1
    $LASTEXITCODE -eq 0
}

Remove-PsmuxSession $T
Remove-PsmuxSession $S
Write-Summary
