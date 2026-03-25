# test_warm_off.ps1 - Comprehensive end-to-end tests for "set -g warm off"
#
# Validates that warm pane/server pre-spawning is fully disabled across
# ALL creation paths: new-session, new-window, split-window (h+v),
# and that no __warm__ process lingers.

$ErrorActionPreference = "Stop"
$PSMUX_DIR = "$env:USERPROFILE\.psmux"

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

function Kill-AllPsmux {
    Get-Process -Name psmux,tmux,pmux -ErrorAction SilentlyContinue |
        Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500
    # Clean stale port files
    Get-ChildItem "$PSMUX_DIR\*.port" -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue
    Get-ChildItem "$PSMUX_DIR\*.key" -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue
}

function Get-WarmPortFiles {
    Get-ChildItem "$PSMUX_DIR\__warm__*.port" -ErrorAction SilentlyContinue
}

function Get-WarmProcesses {
    # Look for psmux processes whose command line contains __warm__
    Get-CimInstance Win32_Process -Filter "Name='psmux.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -match '__warm__' }
}

# ── Setup ──────────────────────────────────────────────────────

Write-Host ""
Write-Host "============================================" -ForegroundColor Cyan
Write-Host " Warm Off: Comprehensive E2E Tests" -ForegroundColor Cyan
Write-Host "============================================" -ForegroundColor Cyan
Write-Host ""

# Clean config and kill everything
Kill-AllPsmux

$configPath = "$env:USERPROFILE\.psmux.conf"
$hadConfig = Test-Path $configPath
if ($hadConfig) {
    $originalConfig = Get-Content $configPath -Raw
}

# Helper: set config for warm-off testing (preserves user lines + adds warm off)
function Set-WarmOff {
    Set-Content $configPath "set -g warm off"
}

# Helper: ensure no warm config (warm defaults to on)
function Set-WarmDefault {
    if ($hadConfig) {
        # Restore original but strip any warm line to test default behavior
        $cleaned = ($originalConfig -split "`n" | Where-Object { $_ -notmatch '^\s*set\s.*\bwarm\b' }) -join "`n"
        Set-Content $configPath $cleaned
    } else {
        Remove-Item $configPath -Force -ErrorAction SilentlyContinue
    }
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
psmux new-session -d -s cfgtest1
Start-Sleep -Seconds 3

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port files after new-session"

$warmProcs = Get-WarmProcesses
Assert-True ($null -eq $warmProcs -or @($warmProcs).Count -eq 0) "No __warm__ processes after new-session"

# ── Test 1.2: New window should NOT spawn warm pane ──
Write-Host ""
Write-Host "Test 1.2: New window with warm off" -ForegroundColor White
psmux new-window -t cfgtest1
Start-Sleep -Seconds 2

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port files after new-window"

$warmProcs = Get-WarmProcesses
Assert-True ($null -eq $warmProcs -or @($warmProcs).Count -eq 0) "No __warm__ processes after new-window"

# ── Test 1.3: Vertical split should NOT spawn warm pane ──
Write-Host ""
Write-Host "Test 1.3: Vertical split with warm off" -ForegroundColor White
psmux split-window -v -t cfgtest1
Start-Sleep -Seconds 2

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port files after split-window -v"

# ── Test 1.4: Horizontal split should NOT spawn warm pane ──
Write-Host ""
Write-Host "Test 1.4: Horizontal split with warm off" -ForegroundColor White
psmux split-window -h -t cfgtest1
Start-Sleep -Seconds 2

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port files after split-window -h"

# ── Test 1.5: Second session should NOT spawn warm server ──
Write-Host ""
Write-Host "Test 1.5: Second session with warm off" -ForegroundColor White
psmux new-session -d -s cfgtest2
Start-Sleep -Seconds 3

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port files after second new-session"

$warmProcs = Get-WarmProcesses
Assert-True ($null -eq $warmProcs -or @($warmProcs).Count -eq 0) "No __warm__ processes after second new-session"

# ── Test 1.6: show-options confirms warm is off ──
Write-Host ""
Write-Host "Test 1.6: Show-options reports warm off" -ForegroundColor White
$warmVal = psmux show-options -g -v warm -t cfgtest1 2>&1
Assert-True ($warmVal -match "off") "show-options -g -v warm returns off"

# Cleanup suite 1
psmux kill-server 2>$null
Start-Sleep -Seconds 1
Kill-AllPsmux

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
psmux new-session -d -s envtest1
Start-Sleep -Seconds 3

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port files with PSMUX_NO_WARM=1"

$warmProcs = Get-WarmProcesses
Assert-True ($null -eq $warmProcs -or @($warmProcs).Count -eq 0) "No __warm__ processes with PSMUX_NO_WARM=1"

# ── Test 2.2: New window with env var ──
Write-Host ""
Write-Host "Test 2.2: New window with PSMUX_NO_WARM=1" -ForegroundColor White
psmux new-window -t envtest1
Start-Sleep -Seconds 2

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port after new-window with env var"

# ── Test 2.3: Split with env var ──
Write-Host ""
Write-Host "Test 2.3: Split with PSMUX_NO_WARM=1" -ForegroundColor White
psmux split-window -h -t envtest1
Start-Sleep -Seconds 2

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__.port after split with env var"

# Cleanup suite 2
psmux kill-server 2>$null
Start-Sleep -Seconds 1
Kill-AllPsmux
Remove-Item env:\PSMUX_NO_WARM -ErrorAction SilentlyContinue

# ══════════════════════════════════════════════════════════════
# TEST SUITE 3: Runtime toggle (warm on -> off -> on)
# ══════════════════════════════════════════════════════════════
Write-Host ""
Write-Host "--- Suite 3: Runtime toggle ---" -ForegroundColor Yellow

# No config override - ensure warm is enabled by default
Set-WarmDefault

# ── Test 3.1: Default (warm on) spawns warm server ──
Write-Host ""
Write-Host "Test 3.1: Default warm on spawns warm" -ForegroundColor White
psmux new-session -d -s toggletest
Start-Sleep -Seconds 5

$warmFiles = Get-WarmPortFiles
Assert-True ($null -ne $warmFiles -and @($warmFiles).Count -gt 0) "Warm port file exists with default (warm on)"

# ── Test 3.2: Toggle warm off at runtime ──
Write-Host ""
Write-Host "Test 3.2: Toggle warm off at runtime" -ForegroundColor White
psmux set-option -g warm off -t toggletest 2>$null
Start-Sleep -Seconds 2

$warmVal = psmux show-options -g -v warm -t toggletest 2>&1
Assert-True ($warmVal -match "off") "show-options reports warm off after runtime set"

# ── Test 3.3: Toggle warm back on at runtime ──
Write-Host ""
Write-Host "Test 3.3: Toggle warm on at runtime" -ForegroundColor White
psmux set-option -g warm on -t toggletest 2>$null
Start-Sleep -Seconds 2

$warmVal = psmux show-options -g -v warm -t toggletest 2>&1
Assert-True ($warmVal -match "on") "show-options reports warm on after runtime re-enable"

# Cleanup suite 3
psmux kill-server 2>$null
Start-Sleep -Seconds 1
Kill-AllPsmux

# ══════════════════════════════════════════════════════════════
# TEST SUITE 4: Chained sessions (spooki44's scenario)
# ══════════════════════════════════════════════════════════════
Write-Host ""
Write-Host "--- Suite 4: Chained sessions (no warm inheritance) ---" -ForegroundColor Yellow

Set-WarmOff

Write-Host ""
Write-Host "Test 4.1: Create 3 sessions, no warm chain" -ForegroundColor White
psmux new-session -d -s chain1
Start-Sleep -Seconds 2
psmux new-session -d -s chain2
Start-Sleep -Seconds 2
psmux new-session -d -s chain3
Start-Sleep -Seconds 2

$warmFiles = Get-WarmPortFiles
Assert-True ($null -eq $warmFiles -or $warmFiles.Count -eq 0) "No __warm__ files after 3 chained sessions"

$warmProcs = Get-WarmProcesses
Assert-True ($null -eq $warmProcs -or @($warmProcs).Count -eq 0) "No __warm__ processes after 3 chained sessions"

# Every session should report warm off
foreach ($s in @("chain1", "chain2", "chain3")) {
    $v = psmux show-options -g -v warm -t $s 2>&1
    Assert-True ($v -match "off") "Session $s reports warm off"
}

# Cleanup suite 4
psmux kill-server 2>$null
Start-Sleep -Seconds 1
Kill-AllPsmux

# ══════════════════════════════════════════════════════════════
# TEST SUITE 5: Default behavior preserved (warm on)
# ══════════════════════════════════════════════════════════════
Write-Host ""
Write-Host "--- Suite 5: Default behavior preserved ---" -ForegroundColor Yellow

# Ensure warm is on by default (no warm config)
Set-WarmDefault

Write-Host ""
Write-Host "Test 5.1: Default config spawns warm server" -ForegroundColor White
psmux new-session -d -s defaulttest
Start-Sleep -Seconds 5

$warmFiles = Get-WarmPortFiles
Assert-True ($null -ne $warmFiles -and @($warmFiles).Count -gt 0) "Warm port file exists with default config"

$warmVal = psmux show-options -g -v warm -t defaulttest 2>&1
Assert-True ($warmVal -match "on") "show-options reports warm on by default"

# Cleanup suite 5
psmux kill-server 2>$null
Start-Sleep -Seconds 1
Kill-AllPsmux

# ── Restore original config ──
if ($hadConfig) {
    Set-Content $configPath $originalConfig
} else {
    Remove-Item $configPath -Force -ErrorAction SilentlyContinue
}

# ── Summary ──
Write-Host ""
Write-Host "============================================" -ForegroundColor Cyan
Write-Host " Results: $pass/$total passed, $fail failed" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Red" })
Write-Host "============================================" -ForegroundColor Cyan

if ($fail -gt 0) { exit 1 }
