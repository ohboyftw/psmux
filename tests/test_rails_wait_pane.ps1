# Rails bench: wait-pane actually sees its -t target.
#
# Before the fix, wait-pane did not work from the CLI at all, not just --ready.
# The `-t` prefilter rebuilds `args` WITHOUT `-t` and its value (comment "Build
# args without -t" in server/connection.rs), and the wait-pane arm then
# re-scanned that already-filtered `args` for "-t". `pane_id` was therefore
# always None, the readiness poll was unreachable, and the arm answered "1".
# The arm now takes the target the prefilter resolved.
#
# The discriminating assertion is ELAPSED TIME, not the exit code. A correct
# timeout and this bug both return rc 1; only the clock separates them. Measured
# on the pre-fix binary (0c8d13d): --timeout 1500 returned rc 1 in 111ms, and
# --ready --timeout 15000 returned rc 1 in 75ms.
. $PSScriptRoot/_harness.ps1

Write-Host "== wait-pane -t target resolution ==" -ForegroundColor Cyan
$S = New-IsolatedSession "waitpane"
Start-Sleep -Seconds 2

$pane = (psmux list-panes -t $S -F '#{pane_id}' 2>$null | Select-Object -First 1)
$Target = "${S}:.$("$pane".Trim())"

function Measure-WaitPane {
    param([string[]]$WaitArgs)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $null = psmux wait-pane @WaitArgs 2>&1
    $rc = $LASTEXITCODE
    $sw.Stop()
    return [PSCustomObject]@{ Rc = $rc; Ms = [int]$sw.ElapsedMilliseconds }
}

# --- case (a): --ready on a settled pane ------------------------------------
# The shell has been idle for ~2s, well past the 500ms output-silence threshold,
# so readiness is already satisfied and this must return promptly with rc 0.
$a = Measure-WaitPane @('-t', $Target, '--ready', '--timeout', '15000')
Test-Case "wait-pane --ready returns 0 on an already-settled pane" {
    $a.Rc -eq 0
}

# --- case (b): the discriminator --------------------------------------------
# A live idle shell never exits, so a plain wait-pane must burn the full timeout
# and THEN report rc 1. Returning rc 1 early is the bug: it means the arm never
# resolved -t and short-circuited without waiting on anything.
$b = Measure-WaitPane @('-t', $Target, '--timeout', '1500')
Test-Case "wait-pane on a live shell times out with rc 1" {
    $b.Rc -eq 1
}
Test-Case "wait-pane actually waits for its timeout (>=1400ms, not ~130ms)" {
    if ($b.Ms -lt 1400) { Write-Host "    elapsed $($b.Ms)ms — arm returned without waiting" -ForegroundColor DarkYellow }
    $b.Ms -ge 1400
}

# --- case (c): a real child exit code ---------------------------------------
# remain-on-exit is required: without it the pane is reaped before wait-pane can
# read the code, and a reaped pane answers 0 (server/mod.rs:328-331) — which
# would pass case (c) for the wrong reason.
$null = psmux set-option -t $S remain-on-exit on 2>&1
$null = psmux split-window -t $S --raw -- cmd /c exit 42 2>&1
Start-Sleep -Milliseconds 800
$dead = (psmux list-panes -t $S -F '#{pane_id}' 2>$null | Select-Object -Last 1)
$DeadTarget = "${S}:.$("$dead".Trim())"
$c = Measure-WaitPane @('-t', $DeadTarget, '--timeout', '10000')
Test-Case "wait-pane reports the child exit code (42)" {
    $c.Rc -eq 42
}

Remove-PsmuxSession $S
Write-Summary
