# Rails bench: paste-buffer delivers the buffer to the TARGET pane.
#
# The server ran `paste-buffer -t P` as two requests: ShowBuffer to fetch the
# text, then SendText to deliver it. The `-t` temp focus is restored after the
# first non-focus request, so ShowBuffer consumed it and SendText went to
# whichever pane was active before — P got nothing and the command exited 0.
# `-p` was parsed by the client and then dropped, so bracketed paste markers
# were never sent either. send-keys -l is the control: one request, delivered.
#
# The recorder is a raw-mode Node process that enables bracketed paste and logs
# every stdin chunk, so the assertions see exactly the bytes the pane received.
. $PSScriptRoot/_harness.ps1

Write-Host "== paste-buffer delivery ==" -ForegroundColor Cyan

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    Test-Case "node is on PATH (recorder needs it)" { $false }
    Write-Summary
}

$work = Join-Path ([IO.Path]::GetTempPath()) ("psmux-paste-" + [guid]::NewGuid().Guid.Substring(0, 8))
New-Item -ItemType Directory -Path $work -Force | Out-Null
$rec = Join-Path $work 'rec.js'
$log = Join-Path $work 'stdin.log'
@'
const fs = require('fs');
const log = process.argv[2];
process.stdin.setRawMode(true);
process.stdout.write('\x1b[?2004h');
process.stdin.on('data', (b) => fs.appendFileSync(log, b));
process.stdin.resume();
fs.writeFileSync(log, 'READY|');
'@ | Set-Content -Path $rec -Encoding ascii

$S = New-IsolatedSession "paste-buffer"
$null = psmux new-window -t $S --raw -- node $rec $log 2>&1

$recPane = $null
$shellPane = $null
$deadline = (Get-Date).AddSeconds(15)
while ((Get-Date) -lt $deadline -and -not $recPane) {
    foreach ($row in (psmux list-panes -a -t $S -F '#{window_index} #{pane_id}' 2>$null)) {
        $bits = "$row".Split(' ')
        if ($bits.Count -ne 2) { continue }
        if ($bits[0] -eq '0') { $shellPane = "${S}:.$($bits[1])" }
        if ($bits[0] -eq '1') { $recPane = "${S}:.$($bits[1])" }
    }
    if (-not $recPane) { Start-Sleep -Milliseconds 300 }
}

function Get-Log {
    if (Test-Path $log) { return [IO.File]::ReadAllText($log) }
    return ''
}

function Wait-Log {
    param([string]$Needle, [int]$TimeoutMs = 5000)
    $deadline = (Get-Date).AddMilliseconds($TimeoutMs)
    while ((Get-Date) -lt $deadline) {
        if ((Get-Log).Contains($Needle)) { return $true }
        Start-Sleep -Milliseconds 200
    }
    return $false
}

Test-Case "recorder pane started in raw mode" {
    $recPane -and (Wait-Log 'READY|' 15000)
}

# The recorder must NOT be the active pane: that is the case the -t routing bug
# hides from. Focus window 0 so every -t below has to move focus temporarily.
$null = psmux select-window -t "${S}:0" 2>&1
Start-Sleep -Milliseconds 300

$null = psmux send-keys -t $recPane -l 'PBthree'
Test-Case "control: send-keys -l reaches the recorder" {
    Wait-Log 'PBthree'
}

$null = psmux set-buffer 'PBone'
$null = psmux paste-buffer -t $recPane
$pasteExit = $LASTEXITCODE
Test-Case "paste-buffer -t delivers the buffer to the target pane" {
    $ok = Wait-Log 'PBone'
    if (-not $ok) { Write-Host "    exit=$pasteExit log=[$(Get-Log)]" -ForegroundColor DarkYellow }
    $ok
}

Test-Case "paste-buffer without -p sends no bracket markers" {
    -not (Get-Log).Contains("`e[200~")
}

Test-Case "paste-buffer -t does not leak into the active pane" {
    $t = (psmux capture-pane -p -t $shellPane 2>$null) -join "`n"
    $t -notmatch 'PBone'
}

$null = psmux set-buffer 'PBtwo'
$null = psmux paste-buffer -p -t $recPane
Test-Case "paste-buffer -p wraps the buffer in bracketed paste markers" {
    $ok = Wait-Log "`e[200~PBtwo`e[201~"
    if (-not $ok) {
        $esc = (Get-Log) -replace "`e", '<ESC>'
        Write-Host "    log=[$esc]" -ForegroundColor DarkYellow
    }
    $ok
}

$null = psmux set-buffer 'PBfirst'
$null = psmux set-buffer 'PBsecond'
$null = psmux paste-buffer -b 1 -t $recPane
Test-Case "paste-buffer -b N pastes that buffer, not the newest" {
    (Wait-Log 'PBfirst') -and -not (Get-Log).Contains('PBsecond')
}

Remove-PsmuxSession $S
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
Write-Summary
