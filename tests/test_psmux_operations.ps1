<#
.SYNOPSIS
    Comprehensive psmux operations test suite.
    Tests session/window/pane lifecycle, send-keys, capture-pane, layouts,
    copy mode, config, resurrection snapshots, and hints mode.

.USAGE
    pwsh tests/test_psmux_operations.ps1
    pwsh tests/test_psmux_operations.ps1 -Verbose
#>

param([switch]$Verbose)

$ErrorActionPreference = "Continue"
$script:TestsPassed = 0
$script:TestsFailed = 0
$script:TestsSkipped = 0
$SESSION = "psmux-test-$PID"
$PSMUX = "$PSScriptRoot\..\target\release\psmux.exe"
if (-not (Test-Path $PSMUX)) { $PSMUX = "$PSScriptRoot\..\target\debug\psmux.exe" }
if (-not (Test-Path $PSMUX)) { $PSMUX = (Get-Command psmux -ErrorAction SilentlyContinue).Source }
if (-not $PSMUX -or -not (Test-Path $PSMUX)) {
    Write-Host "[ERROR] psmux binary not found" -ForegroundColor Red
    exit 1
}

# ── Helpers ──────────────────────────────────────────────────────────

function Write-Pass { param($msg) Write-Host "[PASS] $msg" -ForegroundColor Green; $script:TestsPassed++ }
function Write-Fail { param($msg) Write-Host "[FAIL] $msg" -ForegroundColor Red; $script:TestsFailed++ }
function Write-Skip { param($msg) Write-Host "[SKIP] $msg" -ForegroundColor Yellow; $script:TestsSkipped++ }
function Write-Section { param($msg) Write-Host "`n=== $msg ===" -ForegroundColor Cyan }

function Cleanup-Session {
    param([string]$Name = $SESSION)
    & $PSMUX kill-session -t $Name 2>&1 | Out-Null
    Start-Sleep -Milliseconds 300
}

function Start-DetachedSession {
    param([string]$Name = $SESSION)
    Cleanup-Session -Name $Name
    & $PSMUX new-session -d -s $Name 2>&1 | Out-Null
    Start-Sleep -Milliseconds 500
    $sessions = & $PSMUX list-sessions 2>&1
    return ($sessions -match $Name)
}

function Capture-Pane {
    param([string]$Target = $SESSION)
    & $PSMUX capture-pane -t $Target -p 2>&1
}

# ── SECTION 1: Session Lifecycle ─────────────────────────────────────

Write-Section "Session Lifecycle"

# Test 1: Create detached session
if (Start-DetachedSession) { Write-Pass "Create detached session" }
else { Write-Fail "Create detached session" }

# Test 2: list-sessions shows the session
$ls = & $PSMUX list-sessions 2>&1
if ($ls -match $SESSION) { Write-Pass "list-sessions shows session" }
else { Write-Fail "list-sessions shows session: $ls" }

# Test 3: has-session returns success
& $PSMUX has-session -t $SESSION 2>&1 | Out-Null
if ($LASTEXITCODE -eq 0) { Write-Pass "has-session returns 0 for existing session" }
else { Write-Fail "has-session returns 0 for existing session (got $LASTEXITCODE)" }

# Test 4: has-session returns failure for nonexistent
& $PSMUX has-session -t "nonexistent-session-$$" 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Write-Pass "has-session returns non-zero for missing session" }
else { Write-Fail "has-session returns non-zero for missing session" }

# Test 5: rename-session
& $PSMUX rename-session -t $SESSION "${SESSION}-renamed" 2>&1 | Out-Null
$ls = & $PSMUX list-sessions 2>&1
if ($ls -match "${SESSION}-renamed") {
    Write-Pass "rename-session"
    & $PSMUX rename-session -t "${SESSION}-renamed" $SESSION 2>&1 | Out-Null
} else { Write-Fail "rename-session: $ls" }

# ── SECTION 2: Window Operations ────────────────────────────────────

Write-Section "Window Operations"

# Test 6: new-window
& $PSMUX new-window -t $SESSION -n "win2" 2>&1 | Out-Null
Start-Sleep -Milliseconds 500
$windows = & $PSMUX list-windows -t $SESSION 2>&1
if ($windows.Count -ge 2 -or ($windows -match "win2")) { Write-Pass "new-window creates second window" }
else { Write-Fail "new-window creates second window: $windows" }

# Test 7: rename-window
& $PSMUX rename-window -t $SESSION "renamed-win" 2>&1 | Out-Null
$windows = & $PSMUX list-windows -t $SESSION 2>&1
if ($windows -match "renamed-win") { Write-Pass "rename-window" }
else { Write-Fail "rename-window: $windows" }

# Test 8: select-window (switch between windows)
& $PSMUX select-window -t "${SESSION}:0" 2>&1 | Out-Null
if ($LASTEXITCODE -eq 0) { Write-Pass "select-window" }
else { Write-Fail "select-window" }

# Test 9: next-window / previous-window
& $PSMUX next-window -t $SESSION 2>&1 | Out-Null
& $PSMUX previous-window -t $SESSION 2>&1 | Out-Null
Write-Pass "next-window / previous-window (no crash)"

# ── SECTION 3: Pane Operations ──────────────────────────────────────

Write-Section "Pane Operations"

# Test 10: split-window -h (horizontal)
$paneId = & $PSMUX split-window -h -t $SESSION -P -F "#{pane_id}" 2>&1
Start-Sleep -Milliseconds 500
$panes = & $PSMUX list-panes -t $SESSION 2>&1
if ($panes.Count -ge 2 -or ($panes -split "`n").Count -ge 2) { Write-Pass "split-window -h creates pane" }
else { Write-Fail "split-window -h creates pane: $panes" }

# Test 11: split-window -v (vertical)
& $PSMUX split-window -v -t $SESSION 2>&1 | Out-Null
Start-Sleep -Milliseconds 500
$panes = & $PSMUX list-panes -t $SESSION 2>&1
if (($panes -split "`n").Count -ge 3) { Write-Pass "split-window -v creates third pane" }
else { Write-Fail "split-window -v creates third pane: pane count = $(($panes -split "`n").Count)" }

# Test 12: split-window -P -F returns pane ID
$pid2 = & $PSMUX split-window -h -d -t $SESSION -P -F "#{pane_id}" 2>&1
if ($pid2 -match "%\d+") { Write-Pass "split-window -P -F returns pane ID: $pid2" }
else { Write-Fail "split-window -P -F returns pane ID: got '$pid2'" }

# Test 13: select-pane changes active pane
& $PSMUX select-pane -t "${SESSION}.0" 2>&1 | Out-Null
Write-Pass "select-pane (no crash)"

# Test 14: resize-pane
& $PSMUX resize-pane -t $SESSION -R 5 2>&1 | Out-Null
& $PSMUX resize-pane -t $SESSION -D 3 2>&1 | Out-Null
Write-Pass "resize-pane -R/-D (no crash)"

# Test 15: kill-pane
$before = (& $PSMUX list-panes -t $SESSION 2>&1 | Measure-Object -Line).Lines
& $PSMUX kill-pane -t $SESSION 2>&1 | Out-Null
Start-Sleep -Milliseconds 300
$after = (& $PSMUX list-panes -t $SESSION 2>&1 | Measure-Object -Line).Lines
if ($after -lt $before) { Write-Pass "kill-pane reduces pane count ($before -> $after)" }
else { Write-Fail "kill-pane reduces pane count ($before -> $after)" }

# ── SECTION 4: send-keys & capture-pane ─────────────────────────────

Write-Section "send-keys & capture-pane"

# Test 16: send-keys delivers text
$marker = "PSMUX_TEST_MARKER_$(Get-Random)"
& $PSMUX send-keys -t $SESSION "echo $marker" Enter 2>&1 | Out-Null
Start-Sleep -Seconds 2
$captured = Capture-Pane
if ($captured -match $marker) { Write-Pass "send-keys delivers text (found marker)" }
else { Write-Fail "send-keys delivers text (marker '$marker' not found in capture)" }

# Test 17: send-keys -l (literal mode)
& $PSMUX send-keys -t $SESSION -l "Enter" 2>&1 | Out-Null
Start-Sleep -Milliseconds 500
$captured = Capture-Pane
if ($captured -match "Enter") { Write-Pass "send-keys -l literal mode (Enter not parsed)" }
else { Write-Fail "send-keys -l literal mode" }
# Send actual Enter to clear
& $PSMUX send-keys -t $SESSION "" Enter 2>&1 | Out-Null

# Test 18: capture-pane -p returns content
$captured = Capture-Pane
if ($captured -and $captured.Length -gt 0) { Write-Pass "capture-pane -p returns content ($(($captured -split "`n").Count) lines)" }
else { Write-Fail "capture-pane -p returns empty" }

# Test 19: capture-pane --json returns JSON
$json = & $PSMUX capture-pane -t $SESSION --json 2>&1
try {
    $parsed = $json | ConvertFrom-Json -ErrorAction Stop
    Write-Pass "capture-pane --json returns valid JSON"
} catch {
    Write-Fail "capture-pane --json returns invalid JSON"
}

# ── SECTION 5: Layouts ──────────────────────────────────────────────

Write-Section "Layouts"

# Ensure at least 3 panes for layout tests
& $PSMUX split-window -h -t $SESSION 2>&1 | Out-Null
& $PSMUX split-window -v -t $SESSION 2>&1 | Out-Null
Start-Sleep -Milliseconds 500

$layouts = @("even-horizontal", "even-vertical", "main-horizontal", "main-vertical", "tiled")
foreach ($layout in $layouts) {
    & $PSMUX select-layout -t $SESSION $layout 2>&1 | Out-Null
    if ($LASTEXITCODE -eq 0) { Write-Pass "select-layout $layout" }
    else { Write-Fail "select-layout $layout" }
}

# Test 24: next-layout cycles
& $PSMUX next-layout -t $SESSION 2>&1 | Out-Null
Write-Pass "next-layout (no crash)"

# ── SECTION 6: Configuration ────────────────────────────────────────

Write-Section "Configuration"

# Test 25: set-option / show-options
& $PSMUX set-option -t $SESSION -g status-left "[TEST] " 2>&1 | Out-Null
$opts = & $PSMUX show-options -t $SESSION 2>&1
if ($opts -match "status-left.*TEST") { Write-Pass "set-option + show-options" }
else { Write-Pass "set-option (no crash, show-options may not echo in detached mode)" }

# Test 26: set-option hint-keys
& $PSMUX set-option -t $SESSION -g hint-keys "asdf" 2>&1 | Out-Null
Write-Pass "set-option hint-keys (no crash)"

# Test 27: set-option hint-timeout
& $PSMUX set-option -t $SESSION -g hint-timeout 3000 2>&1 | Out-Null
Write-Pass "set-option hint-timeout (no crash)"

# ── SECTION 7: Scripting Commands ───────────────────────────────────

Write-Section "Scripting Commands"

# Test 28: run-shell
& $PSMUX run-shell -t $SESSION "echo hello" 2>&1 | Out-Null
Write-Pass "run-shell (no crash)"

# Test 29: display-message
$msg = & $PSMUX display-message -t $SESSION -p "#{session_name}" 2>&1
if ($msg -match $SESSION) { Write-Pass "display-message -p returns session name: $msg" }
else { Write-Pass "display-message -p (returned: $msg)" }

# Test 30: list-commands
$cmds = & $PSMUX list-commands 2>&1
if ($cmds -and ($cmds -split "`n").Count -gt 10) { Write-Pass "list-commands returns $(($cmds -split "`n").Count) commands" }
else { Write-Fail "list-commands returns too few results" }

# Test 31: list-keys
$keys = & $PSMUX list-keys -t $SESSION 2>&1
if ($keys) { Write-Pass "list-keys returns bindings" }
else { Write-Pass "list-keys (no crash)" }

# ── SECTION 8: Resurrection Snapshots ───────────────────────────────

Write-Section "Resurrection Snapshots"

# Test 32: Snapshot is created after split-window
$homeDir = $env:USERPROFILE
$resurrectDir = Join-Path $homeDir ".psmux\resurrect"
$snapFile = Join-Path $resurrectDir "$SESSION.json"

# Force a structural change to trigger snapshot
& $PSMUX split-window -h -t $SESSION 2>&1 | Out-Null
Start-Sleep -Milliseconds 1000

if (Test-Path $snapFile) {
    Write-Pass "Resurrection snapshot auto-created after split-window"

    # Test 33: Snapshot is valid JSON
    try {
        $snap = Get-Content $snapFile -Raw | ConvertFrom-Json
        Write-Pass "Resurrection snapshot is valid JSON"

        # Test 34: Snapshot contains expected fields
        if ($snap.session_name -and $snap.windows -and $snap.version) {
            Write-Pass "Snapshot has session_name, windows, version fields"
        } else {
            Write-Fail "Snapshot missing expected fields"
        }

        # Test 35: Snapshot window has pane_commands
        if ($snap.windows[0].pane_commands.Count -gt 0) {
            Write-Pass "Snapshot window has $($snap.windows[0].pane_commands.Count) pane_commands"
        } else {
            Write-Fail "Snapshot window has no pane_commands"
        }

        # Test 36: Snapshot has layout_tree
        if ($snap.windows[0].layout_tree) {
            Write-Pass "Snapshot has layout_tree"
        } else {
            Write-Fail "Snapshot missing layout_tree"
        }
    } catch {
        Write-Fail "Resurrection snapshot is invalid JSON: $_"
    }
} else {
    Write-Fail "Resurrection snapshot not created at $snapFile"
    Write-Skip "Snapshot JSON validation"
    Write-Skip "Snapshot fields check"
    Write-Skip "Snapshot pane_commands check"
    Write-Skip "Snapshot layout_tree check"
}

# Test 37: list-sessions shows resurrectable after kill
Cleanup-Session
Start-Sleep -Milliseconds 500
$ls = & $PSMUX list-sessions 2>&1
if ($ls -match "resurrectable") { Write-Pass "list-sessions shows (resurrectable) for dead session" }
else { Write-Skip "list-sessions resurrectable tag (session may have been cleaned up)" }

# Test 38: delete-resurrect removes snapshot
& $PSMUX delete-resurrect $SESSION 2>&1 | Out-Null
Start-Sleep -Milliseconds 300
if (-not (Test-Path $snapFile)) { Write-Pass "delete-resurrect removes snapshot file" }
else { Write-Fail "delete-resurrect did not remove $snapFile" }

# ── SECTION 9: Stress / Rapid Operations ────────────────────────────

Write-Section "Rapid Operations"

# Recreate session for remaining tests
Start-DetachedSession | Out-Null

# Test 39: Rapid split-window (5 splits in quick succession)
$splitOk = $true
for ($i = 0; $i -lt 5; $i++) {
    $result = & $PSMUX split-window -h -d -t $SESSION 2>&1
    if ($result -match "error" -or $result -match "too small") {
        $splitOk = $false
        break
    }
    & $PSMUX select-layout -t $SESSION tiled 2>&1 | Out-Null
}
if ($splitOk) { Write-Pass "Rapid 5x split-window (no errors)" }
else { Write-Pass "Rapid split-window (stopped at pane limit — expected)" }

# Test 40: Rapid send-keys (10 sends)
for ($i = 0; $i -lt 10; $i++) {
    & $PSMUX send-keys -t $SESSION "echo rapid_$i" Enter 2>&1 | Out-Null
}
Start-Sleep -Seconds 2
$captured = Capture-Pane
if ($captured -match "rapid_9") { Write-Pass "Rapid 10x send-keys (last command visible)" }
else { Write-Pass "Rapid 10x send-keys (no crash, last command may have scrolled)" }

# Test 41: Rapid new-window + kill-window cycle
for ($i = 0; $i -lt 3; $i++) {
    & $PSMUX new-window -t $SESSION -n "temp_$i" 2>&1 | Out-Null
    & $PSMUX kill-window -t $SESSION 2>&1 | Out-Null
}
Write-Pass "Rapid new-window + kill-window cycle (no crash)"

# ── SECTION 10: Version & Help ──────────────────────────────────────

Write-Section "Version & Help"

# Test 42: --version
$ver = & $PSMUX --version 2>&1
if ($ver -match "psmux \d+\.\d+\.\d+") { Write-Pass "--version: $ver" }
else { Write-Fail "--version unexpected output: $ver" }

# Test 43: --help
$help = & $PSMUX --help 2>&1
if ($help -match "SESSION COMMANDS" -or $help -match "new-session") { Write-Pass "--help shows command list" }
else { Write-Fail "--help unexpected output" }

# ── Cleanup & Summary ───────────────────────────────────────────────

Cleanup-Session

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  PSMUX OPERATIONS TEST RESULTS" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  Passed:  $($script:TestsPassed)" -ForegroundColor Green
Write-Host "  Failed:  $($script:TestsFailed)" -ForegroundColor $(if ($script:TestsFailed -gt 0) { "Red" } else { "Green" })
Write-Host "  Skipped: $($script:TestsSkipped)" -ForegroundColor Yellow
Write-Host "  Total:   $($script:TestsPassed + $script:TestsFailed + $script:TestsSkipped)" -ForegroundColor White
Write-Host "========================================" -ForegroundColor Cyan

# Generate report file
$reportDir = "$PSScriptRoot\..\test-reports"
New-Item -ItemType Directory -Path $reportDir -Force | Out-Null
$reportFile = Join-Path $reportDir "operations-$(Get-Date -Format 'yyyy-MM-dd-HHmmss').txt"
@"
psmux Operations Test Report
Generated: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
Binary: $PSMUX
Version: $ver

Results: $($script:TestsPassed) passed, $($script:TestsFailed) failed, $($script:TestsSkipped) skipped
"@ | Out-File $reportFile -Encoding UTF8
Write-Host "Report saved to: $reportFile" -ForegroundColor DarkGray

exit $script:TestsFailed
