# Feature bench: mycel event bus (psmux/* topics)
# The mycel feature is on by default and psmux starts its own bus. This test
# gives the session a private bus on a free port with a temp discovery file,
# so it never touches the user's bus on 4321 or ~/.mycel/server, and stops
# that bus at the end.
. $PSScriptRoot/_harness.ps1

Write-Host "== mycel events ==" -ForegroundColor Cyan

$probe = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$probe.Start()
$busPort = $probe.LocalEndpoint.Port
$probe.Stop()
$env:PSMUX_BUS = $null
$env:PSMUX_BUS_PORT = "$busPort"
$env:PSMUX_BUS_DISCOVERY = Join-Path $env:TEMP "psmux-test-bus-$PID\server"

$S = New-IsolatedSession "mycel-events"

Test-Case "spawning a pane does not fail when mycel is active" {
    # Indirect: if mycel publish hooks panicked, the pane spawn would error.
    $before = (psmux list-panes -t $S 2>$null | Measure-Object -Line).Lines
    $null = psmux split-window -v -t $S 2>&1
    Start-Sleep -Milliseconds 400
    $after = (psmux list-panes -t $S 2>$null | Measure-Object -Line).Lines
    $after -gt $before
}

Test-Case "killing a pane does not fail when mycel publishes pane/exited" {
    $null = psmux kill-pane -t $S 2>&1
    Start-Sleep -Milliseconds 300
    $LASTEXITCODE -eq 0
}

Remove-PsmuxSession $S
# Let the dying server publish its last event first, or it would start the
# private bus again after the stop.
Start-Sleep -Milliseconds 500
$null = psmux bus stop 2>&1
Write-Summary
