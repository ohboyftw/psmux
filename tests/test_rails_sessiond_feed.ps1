# Rails bench: the sessiond cockpit feed (#{sessiond_*} format variables).
#
# `@sessiond-feed` names a command that prints sessiond's state-topic JSONL
# envelopes. psmux runs it on first use and keeps the latest state per session.
# The fixture covers the contract's edge cases: a stale row before the sidecar's
# started marker, a state change, a removal, a heartbeat, a malformed line, and
# a message on another topic.
. $PSScriptRoot/_harness.ps1

Write-Host "== sessiond feed ==" -ForegroundColor Cyan

function Read-Var {
    param([string]$Session, [string]$Name)
    ((psmux display-message -t $Session -p "#{$Name}" 2>$null) -join '').Trim()
}

$fixture = Join-Path ([IO.Path]::GetTempPath()) "psmux-sessiond-feed-$PID.jsonl"
@(
    '{"topic":"sessiond/session/state","payload":{"session":"old","state":"blocked","since":"2026-10-21T20:00:00.000Z","reason":"pending:command"}}'
    '{"topic":"sessiond/sidecar/started","payload":{"at":"2026-10-21T20:39:00.000Z","pid":4242}}'
    '{"topic":"sessiond/session/state","payload":{"session":"s1","state":"working","since":"2026-10-21T20:40:00.000Z","reason":"status:running","title":"fix the bug","project":"psmux"}}'
    '{"topic":"sessiond/session/state","payload":{"session":"s1","state":"blocked","since":"2026-10-21T20:41:07.512Z","reason":"pending:command","title":"fix the bug","project":"psmux"}}'
    '{"topic":"sessiond/session/state","payload":{"session":"s2","state":"working","since":"2026-10-21T20:41:00.000Z","reason":"activity:running","title":"refactor","project":"layax"}}'
    '{"topic":"sessiond/session/state","payload":{"session":"s3","state":"done","since":"2026-10-21T20:42:00.000Z","reason":"status:completed"}}'
    '{"topic":"sessiond/session/removed","payload":{"session":"s3","since":"2026-10-21T20:45:00.004Z","reason":"archived"}}'
    '{"topic":"sessiond/session/state","payload":{"session":"s4","state":"done","since":"2026-10-21T20:43:00.000Z","reason":"status:completed"}}'
    '{"topic":"sessiond/sidecar/alive","payload":{"at":"2026-10-21T20:45:30.000Z","started":"2026-10-21T20:39:00.000Z","sessions":3}}'
    '{"topic":"sessiond/session/state","payload":'
    '{"topic":"psmux/pane/exited","payload":{"session":"s5","state":"error"}}'
) | Set-Content -LiteralPath $fixture -Encoding utf8

Test-Case "the feed reads off when @sessiond-feed is unset" {
    $S = New-IsolatedSession "sessiond-off"
    $feed = Read-Var $S 'sessiond_feed'
    $summary = Read-Var $S 'sessiond_summary'
    Remove-PsmuxSession $S
    ($feed -eq 'off') -and ($summary -eq '')
}

$S = New-IsolatedSession "sessiond-on"
# The sleep keeps the feed command running, as `tail -F` would.
$null = psmux set-option -t $S -g '@sessiond-feed' "Get-Content -LiteralPath '$fixture'; Start-Sleep -Seconds 30" 2>&1
# The first expansion starts the feed; give its PowerShell time to start.
$null = Read-Var $S 'sessiond_summary'
$deadline = (Get-Date).AddSeconds(20)
$summary = ''
while ((Get-Date) -lt $deadline) {
    $summary = Read-Var $S 'sessiond_summary'
    if ($summary -eq 'B1 W1 D1') { break }
    Start-Sleep -Milliseconds 300
}

Test-Case "the summary counts the latest state per session, most urgent first" {
    if ($summary -ne 'B1 W1 D1') { Write-Host "    summary was '$summary'" }
    $summary -eq 'B1 W1 D1'
}

Test-Case "rows from before the started marker and removed sessions are not counted" {
    (Read-Var $S 'sessiond_total') -eq '3'
}

Test-Case "a per-state count reads the board" {
    (Read-Var $S 'sessiond_blocked') -eq '1'
}

Test-Case "attention names the blocked session by project and title" {
    $attention = Read-Var $S 'sessiond_attention'
    if ($attention -ne 'psmux: fix the bug') { Write-Host "    attention was '$attention'" }
    $attention -eq 'psmux: fix the bug'
}

Test-Case "the feed reads live while its command runs and the sidecar was heard" {
    $feed = Read-Var $S 'sessiond_feed'
    if ($feed -ne 'live') { Write-Host "    feed was '$feed'" }
    $feed -eq 'live'
}

Remove-PsmuxSession $S
Remove-Item -LiteralPath $fixture -ErrorAction SilentlyContinue
Write-Summary
