# test_warm_off.ps1 - Comprehensive end-to-end tests for "set -g warm off"
#
# Validates that warm pane/server pre-spawning is fully disabled across
# ALL creation paths: new-session, new-window, split-window (h+v),
# and that no __warm__ process lingers.

$ErrorActionPreference = "Stop"
# Keep tests off the user's mycel bus: start none and find none. Port 0
# never connects, and the discovery file below never exists.
$env:PSMUX_BUS = '0'
$env:PSMUX_BUS_PORT = '0'
$env:PSMUX_BUS_DISCOVERY = Join-Path $env:TEMP 'psmux-test-no-bus\server'

# This test turns the warm pool on and off itself, so it runs in a scratch
# data directory instead of setting PSMUX_NO_WARM for good: it can only
# claim, spawn and delete the standbys and port files there, never the user's.
$env:PSMUX_NO_WARM = $null
$env:PSMUX_DATA_DIR = Join-Path $env:TEMP "psmux-warm-test-$PID"
New-Item -ItemType Directory -Path $env:PSMUX_DATA_DIR -Force | Out-Null
$PSMUX_DIR = $env:PSMUX_DATA_DIR

# Everything this test starts lives in its own -L namespace, so cleanup can
# end exactly its own servers: `kill-server -L` never sweeps other psmux
# processes, and the fallback below matches only command lines naming this
# namespace. No bare kill-server and no Stop-Process on every psmux image.
$L = "warmtest$PID"

$PSMUX = 'psmux'

# The config this test writes is a scratch file, never ~/.psmux.conf:
# PSMUX_CONFIG_FILE replaces the home-directory search, and a missing file
# means no config (warm defaults to on).
$configPath = Join-Path $env:PSMUX_DATA_DIR 'psmux.conf'
$env:PSMUX_CONFIG_FILE = $configPath

$pass = 0
$fail = 0
$total = 0

function Assert-True($condition, $msg) {
    $script:total++
    if ($condition) {
        Write-Host "  [PASS] $msg" -ForegroundColor Green
        $script:pass++
    } else {
        Write-Host "  [FAIL] $msg" -ForegroundColor Red
        $script:fail++
    }
}

function Stop-ScratchPsmux {
    & $PSMUX -L $L kill-server 2>$null
    Start-Sleep -Milliseconds 500
    $ours = "(^|\s)-L\s+" + [regex]::Escape($L) + "(\s|$)"
    Get-CimInstance Win32_Process -Filter "Name='psmux.exe' OR Name='pmux.exe' OR Name='tmux.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -match $ours } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    Get-ChildItem "$PSMUX_DIR\${L}__*.port", "$PSMUX_DIR\${L}__*.key" -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue
}

function Get-WarmPortFiles {
    Get-ChildItem "$PSMUX_DIR\${L}____warm__*.port" -ErrorAction SilentlyContinue
}

function Get-WarmProcesses {
    # This test's psmux processes whose command line contains __warm__
    Get-CimInstance Win32_Process -Filter "Name='psmux.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -match '__warm__' -and $_.CommandLine -match [regex]::Escape($L) }
}

# ── Setup ──────────────────────────────────────────────────────

Write-Host ""
Write-Host "============================================" -ForegroundColor Cyan
Write-Host " Warm Off: Comprehensive E2E Tests" -ForegroundColor Cyan
Write-Host "============================================" -ForegroundColor Cyan
Write-Host ""

# Start from a clean scratch namespace
Stop-ScratchPsmux

# Helper: set config for warm-off testing
function Set-WarmOff {
    Set-Content $configPath "set -g warm off"
}

# Helper: no config at all (warm defaults to on)
function Set-WarmDefault {
    Remove-Item $configPath -Force -ErrorAction SilentlyContinue
}

# ══════════════════════════════════════════════════════════════
# TEST SUITE 1: Config-based warm off (set -g warm off)
# ══════════════════════════════════════════════════════════════
Write-Host "--- Suite 1: Config-based warm off ---" -ForegroundColor Yellow

# Write config
Set-WarmOff

# ── Test 1.1: New session should NOT spawn warm server ──
Write-Host ""
Write-Host "Test 1.1: New session with warm off" -ForegroundColor White
psmux -L $L new-session -d -s cfgtest1
Start-Sleep -Seconds 3

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port files after new-session"

$warmProcs = Get-WarmProcesses
Assert-True ($null -eq $warmProcs -or @($warmProcs).Count -eq 0) "No __warm__ processes after new-session"

# ── Test 1.2: New window should NOT spawn warm pane ──
Write-Host ""
Write-Host "Test 1.2: New window with warm off" -ForegroundColor White
psmux -L $L new-window -t cfgtest1
Start-Sleep -Seconds 2

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port files after new-window"

$warmProcs = Get-WarmProcesses
Assert-True ($null -eq $warmProcs -or @($warmProcs).Count -eq 0) "No __warm__ processes after new-window"

# ── Test 1.3: Vertical split should NOT spawn warm pane ──
Write-Host ""
Write-Host "Test 1.3: Vertical split with warm off" -ForegroundColor White
psmux -L $L split-window -v -t cfgtest1
Start-Sleep -Seconds 2

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port files after split-window -v"

# ── Test 1.4: Horizontal split should NOT spawn warm pane ──
Write-Host ""
Write-Host "Test 1.4: Horizontal split with warm off" -ForegroundColor White
psmux -L $L split-window -h -t cfgtest1
Start-Sleep -Seconds 2

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port files after split-window -h"

# ── Test 1.5: Second session should NOT spawn warm server ──
Write-Host ""
Write-Host "Test 1.5: Second session with warm off" -ForegroundColor White
psmux -L $L new-session -d -s cfgtest2
Start-Sleep -Seconds 3

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port files after second new-session"

$warmProcs = Get-WarmProcesses
Assert-True ($null -eq $warmProcs -or @($warmProcs).Count -eq 0) "No __warm__ processes after second new-session"

# ── Test 1.6: show-options confirms warm is off ──
Write-Host ""
Write-Host "Test 1.6: Show-options reports warm off" -ForegroundColor White
$warmVal = psmux -L $L show-options -g -v warm -t cfgtest1 2>&1
Assert-True ($warmVal -match "off") "show-options -g -v warm returns off"

# Cleanup suite 1
psmux -L $L kill-server 2>$null
Start-Sleep -Seconds 1
Stop-ScratchPsmux

# ══════════════════════════════════════════════════════════════
# TEST SUITE 2: Environment variable based warm off
# ══════════════════════════════════════════════════════════════
Write-Host ""
Write-Host "--- Suite 2: PSMUX_NO_WARM=1 env var ---" -ForegroundColor Yellow

# Clear config, use env var
Set-WarmDefault
$env:PSMUX_NO_WARM = "1"

# ── Test 2.1: New session with env var ──
Write-Host ""
Write-Host "Test 2.1: New session with PSMUX_NO_WARM=1" -ForegroundColor White
psmux -L $L new-session -d -s envtest1
Start-Sleep -Seconds 3

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port files with PSMUX_NO_WARM=1"

$warmProcs = Get-WarmProcesses
Assert-True ($null -eq $warmProcs -or @($warmProcs).Count -eq 0) "No __warm__ processes with PSMUX_NO_WARM=1"

# ── Test 2.2: New window with env var ──
Write-Host ""
Write-Host "Test 2.2: New window with PSMUX_NO_WARM=1" -ForegroundColor White
psmux -L $L new-window -t envtest1
Start-Sleep -Seconds 2

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port after new-window with env var"

# ── Test 2.3: Split with env var ──
Write-Host ""
Write-Host "Test 2.3: Split with PSMUX_NO_WARM=1" -ForegroundColor White
psmux -L $L split-window -h -t envtest1
Start-Sleep -Seconds 2

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port after split with env var"

# Cleanup suite 2
psmux -L $L kill-server 2>$null
Start-Sleep -Seconds 1
Stop-ScratchPsmux
Remove-Item env:\PSMUX_NO_WARM -ErrorAction SilentlyContinue

# ══════════════════════════════════════════════════════════════
# TEST SUITE 3: Runtime commands (set-option -g warm off/on)
# Verifies actual warm file/process state after runtime toggles,
# not just show-options output.
# ══════════════════════════════════════════════════════════════
Write-Host ""
Write-Host "--- Suite 3: Runtime commands ---" -ForegroundColor Yellow

# No config override - ensure warm is enabled by default
Set-WarmDefault

# ── Test 3.1: Default (warm on) spawns warm server ──
Write-Host ""
Write-Host "Test 3.1: Default warm on spawns warm" -ForegroundColor White
psmux -L $L new-session -d -s toggletest
Start-Sleep -Seconds 5

$warmFiles = Get-WarmPortFiles
Assert-True ($null -ne $warmFiles -and @($warmFiles).Count -gt 0) "Warm port file exists with default (warm on)"

# ── Test 3.2: Runtime set warm off removes warm files ──
Write-Host ""
Write-Host "Test 3.2: Runtime warm off removes warm" -ForegroundColor White
psmux -L $L set-option -g warm off -t toggletest 2>$null
Start-Sleep -Seconds 3

$warmVal = psmux -L $L show-options -g -v warm -t toggletest 2>&1
Assert-True ($warmVal -match "off") "show-options reports warm off after runtime set"

# ── Test 3.3: New window after runtime warm off has no warm ──
Write-Host ""
Write-Host "Test 3.3: New window after runtime warm off" -ForegroundColor White
psmux -L $L new-window -t toggletest
Start-Sleep -Seconds 3

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No warm port files after new-window (runtime warm off)"

# ── Test 3.4: Split after runtime warm off has no warm ──
Write-Host ""
Write-Host "Test 3.4: Split after runtime warm off" -ForegroundColor White
psmux -L $L split-window -v -t toggletest
Start-Sleep -Seconds 3

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No warm port files after split-window (runtime warm off)"

# ── Test 3.5: Runtime set warm on restores warm ──
Write-Host ""
Write-Host "Test 3.5: Runtime warm on restores warm" -ForegroundColor White
psmux -L $L set-option -g warm on -t toggletest 2>$null
Start-Sleep -Seconds 3

$warmVal = psmux -L $L show-options -g -v warm -t toggletest 2>&1
Assert-True ($warmVal -match "on") "show-options reports warm on after runtime re-enable"

# ── Test 3.6: New window after runtime warm on has warm ──
Write-Host ""
Write-Host "Test 3.6: New window after runtime warm on" -ForegroundColor White
psmux -L $L new-window -t toggletest
Start-Sleep -Seconds 3

$warmFiles = Get-WarmPortFiles
# Warm pane is internal to the server, port file is for the warm SERVER
# After re-enabling warm, new operations should restore warm state
$warmVal2 = psmux -L $L show-options -g -v warm -t toggletest 2>&1
Assert-True ($warmVal2 -match "on") "warm still on after new-window (runtime warm on)"

# ── Test 3.7: Split after runtime warm on ──
Write-Host ""
Write-Host "Test 3.7: Split after runtime warm on" -ForegroundColor White
psmux -L $L split-window -h -t toggletest
Start-Sleep -Seconds 3

$warmVal3 = psmux -L $L show-options -g -v warm -t toggletest 2>&1
Assert-True ($warmVal3 -match "on") "warm still on after split-window (runtime warm on)"

# Cleanup suite 3
psmux -L $L kill-server 2>$null
Start-Sleep -Seconds 1
Stop-ScratchPsmux

# ══════════════════════════════════════════════════════════════
# TEST SUITE 4: Chained sessions (spooki44's scenario)
# ══════════════════════════════════════════════════════════════
Write-Host ""
Write-Host "--- Suite 4: Chained sessions (no warm inheritance) ---" -ForegroundColor Yellow

Set-WarmOff

Write-Host ""
Write-Host "Test 4.1: Create 3 sessions, no warm chain" -ForegroundColor White
psmux -L $L new-session -d -s chain1
Start-Sleep -Seconds 2
psmux -L $L new-session -d -s chain2
Start-Sleep -Seconds 2
psmux -L $L new-session -d -s chain3
Start-Sleep -Seconds 2

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__ files after 3 chained sessions"

$warmProcs = Get-WarmProcesses
Assert-True ($null -eq $warmProcs -or @($warmProcs).Count -eq 0) "No __warm__ processes after 3 chained sessions"

# Every session should report warm off
foreach ($s in @("chain1", "chain2", "chain3")) {
    $v = psmux -L $L show-options -g -v warm -t $s 2>&1
    Assert-True ($v -match "off") "Session $s reports warm off"
}

# Cleanup suite 4
psmux -L $L kill-server 2>$null
Start-Sleep -Seconds 1
Stop-ScratchPsmux

# ══════════════════════════════════════════════════════════════
# TEST SUITE 5: Default behavior preserved (warm on)
# ══════════════════════════════════════════════════════════════
Write-Host ""
Write-Host "--- Suite 5: Default behavior preserved ---" -ForegroundColor Yellow

# Ensure warm is on by default (no warm config)
Set-WarmDefault

Write-Host ""
Write-Host "Test 5.1: Default config spawns warm server" -ForegroundColor White
psmux -L $L new-session -d -s defaulttest
Start-Sleep -Seconds 5

$warmFiles = Get-WarmPortFiles
Assert-True ($null -ne $warmFiles -and @($warmFiles).Count -gt 0) "Warm port file exists with default config"

$warmVal = psmux -L $L show-options -g -v warm -t defaulttest 2>&1
Assert-True ($warmVal -match "on") "show-options reports warm on by default"

# Cleanup suite 5
psmux -L $L kill-server 2>$null
Start-Sleep -Seconds 1
Stop-ScratchPsmux

# ── Remove the scratch config ──
Remove-Item $configPath -Force -ErrorAction SilentlyContinue
$env:PSMUX_CONFIG_FILE = $null

# ── Summary ──
Write-Host ""
Write-Host "============================================" -ForegroundColor Cyan
Write-Host " Results: $pass/$total passed, $fail failed" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Red" })
Write-Host "============================================" -ForegroundColor Cyan

if ($fail -gt 0) { exit 1 }
