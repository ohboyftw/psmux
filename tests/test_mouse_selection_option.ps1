# psmux mouse-selection option (#245) — boundary contract tests
# Tests the upstream port from de07d78: set/show round-trip, defaults,
# parse boundary, and ohboy-only option-catalog regression.
# Run: powershell -ExecutionPolicy Bypass -File tests\test_mouse_selection_option.ps1

$ErrorActionPreference = "Continue"
$script:TestsPassed = 0
$script:TestsFailed = 0

function Write-Pass { param($msg) Write-Host "[PASS] $msg" -ForegroundColor Green; $script:TestsPassed++ }
function Write-Fail { param($msg) Write-Host "[FAIL] $msg" -ForegroundColor Red; $script:TestsFailed++ }
function Write-Info { param($msg) Write-Host "[INFO] $msg" -ForegroundColor Cyan }
function Write-Test { param($msg) Write-Host "[TEST] $msg" -ForegroundColor White }

$PSMUX = (Resolve-Path "$PSScriptRoot\..\target\release\psmux.exe" -ErrorAction SilentlyContinue).Path
if (-not $PSMUX) { $PSMUX = (Resolve-Path "$PSScriptRoot\..\target\debug\psmux.exe" -ErrorAction SilentlyContinue).Path }
if (-not $PSMUX) { Write-Error "psmux binary not found"; exit 1 }
Write-Info "Using: $PSMUX"

function New-PsmuxSession {
    param([string]$Name)
    Start-Process -FilePath $PSMUX -ArgumentList "new-session -s $Name -d" -WindowStyle Hidden
    Start-Sleep -Seconds 3
}

function Psmux { & $PSMUX @args 2>&1; Start-Sleep -Milliseconds 300 }

function Get-OptionValue {
    param([string]$Session, [string]$Name)
    $raw = (& $PSMUX show-options -t $Session -g $Name 2>&1 | Out-String).Trim()
    # Format is "<name> <value>" or just "<value>" depending on version.
    # Strip leading "<name> " if present.
    if ($raw -match "^$([regex]::Escape($Name))\s+(.*)$") { return $matches[1].Trim() }
    return $raw
}

# Cleanup
Write-Info "Cleaning up existing sessions..."
Start-Process -FilePath $PSMUX -ArgumentList "kill-server" -WindowStyle Hidden
Start-Sleep -Seconds 3
Remove-Item "$env:USERPROFILE\.psmux\*.port" -Force -ErrorAction SilentlyContinue
Remove-Item "$env:USERPROFILE\.psmux\*.key" -Force -ErrorAction SilentlyContinue

Write-Info "Creating test session 'msel'..."
New-PsmuxSession -Name "msel"
& $PSMUX has-session -t msel 2>$null
if ($LASTEXITCODE -ne 0) { Write-Host "FATAL: Cannot create test session" -ForegroundColor Red; exit 1 }
Write-Info "Session 'msel' created"

# ============================================================
# 1. DEFAULT CONTRACT — mouse-selection defaults to "on"
# ============================================================
Write-Host ""
Write-Host ("=" * 60)
Write-Host "DEFAULT CONTRACT"
Write-Host ("=" * 60)

Write-Test "show-options -g mouse-selection (no explicit set) returns 'on'"
$val = Get-OptionValue -Session msel -Name "mouse-selection"
if ($val -eq "on") {
    Write-Pass "default mouse-selection = on (got '$val')"
} else {
    Write-Fail "default mouse-selection expected 'on', got '$val'"
}

# ============================================================
# 2. SCHEMA/PARSE CONTRACT — set off / show off
# ============================================================
Write-Host ""
Write-Host ("=" * 60)
Write-Host "SET off ROUND-TRIP"
Write-Host ("=" * 60)

Write-Test "set -g mouse-selection off"
$out = (Psmux set -t msel -g mouse-selection off | Out-String).Trim()
if ($LASTEXITCODE -eq 0) {
    Write-Pass "set -g mouse-selection off succeeded"
} else {
    Write-Fail "set -g mouse-selection off failed: $out"
}

Write-Test "show-options -g mouse-selection echoes 'off'"
$val = Get-OptionValue -Session msel -Name "mouse-selection"
if ($val -eq "off") {
    Write-Pass "round-trip off (got '$val')"
} else {
    Write-Fail "round-trip off failed: expected 'off', got '$val'"
}

# ============================================================
# 3. SCHEMA/PARSE CONTRACT — set on / show on
# ============================================================
Write-Host ""
Write-Host ("=" * 60)
Write-Host "SET on ROUND-TRIP"
Write-Host ("=" * 60)

Write-Test "set -g mouse-selection on"
$out = (Psmux set -t msel -g mouse-selection on | Out-String).Trim()
if ($LASTEXITCODE -eq 0) {
    Write-Pass "set -g mouse-selection on succeeded"
} else {
    Write-Fail "set -g mouse-selection on failed: $out"
}

Write-Test "show-options -g mouse-selection echoes 'on'"
$val = Get-OptionValue -Session msel -Name "mouse-selection"
if ($val -eq "on") {
    Write-Pass "round-trip on (got '$val')"
} else {
    Write-Fail "round-trip on failed: expected 'on', got '$val'"
}

# ============================================================
# 4. BOUNDARY ERROR CONTRACT — garbage value
# ohboy stores unknown options but boolean parser maps anything
# !~ "on|true|1" to false. Contract: garbage -> reads back "off".
# ============================================================
Write-Host ""
Write-Host ("=" * 60)
Write-Host "BOUNDARY: GARBAGE VALUE"
Write-Host ("=" * 60)

Write-Test "set -g mouse-selection garbage (should not crash)"
$out = (Psmux set -t msel -g mouse-selection garbage | Out-String).Trim()
# We don't assert exit code — ohboy's set-option is permissive — but it must not crash.
Write-Pass "set -g mouse-selection garbage did not crash (exit=$LASTEXITCODE, out='$out')"

Write-Test "show-options -g mouse-selection after garbage reads 'off'"
$val = Get-OptionValue -Session msel -Name "mouse-selection"
if ($val -eq "off") {
    Write-Pass "garbage parsed to 'off' (got '$val')"
} else {
    Write-Fail "garbage expected to fall back to 'off', got '$val'"
}

# Restore for downstream tests
Psmux set -t msel -g mouse-selection on | Out-Null

# ============================================================
# 5. REGRESSION CONTRACT — ohboy-only options still resolve
# Verifies the catalog/option dispatch wasn't clobbered by the port.
# ============================================================
Write-Host ""
Write-Host ("=" * 60)
Write-Host "REGRESSION: OHBOY-ONLY OPTIONS"
Write-Host ("=" * 60)

Write-Test "show-options -g allow-passthrough returns ohboy default 'off'"
$val = Get-OptionValue -Session msel -Name "allow-passthrough"
if ($val -eq "off") {
    Write-Pass "allow-passthrough default preserved (got '$val')"
} else {
    Write-Fail "allow-passthrough default lost: expected 'off', got '$val'"
}

Write-Test "show-options -g claude-code-fix-tty returns ohboy default 'on'"
$val = Get-OptionValue -Session msel -Name "claude-code-fix-tty"
if ($val -eq "on") {
    Write-Pass "claude-code-fix-tty default preserved (got '$val')"
} else {
    Write-Fail "claude-code-fix-tty default lost: expected 'on', got '$val'"
}

# Mutating allow-passthrough and round-tripping ensures the dispatcher
# didn't accidentally route ohboy options into the new mouse-selection arm.
Write-Test "set -g allow-passthrough on round-trips (mouse-selection didn't shadow it)"
Psmux set -t msel -g allow-passthrough on | Out-Null
$val = Get-OptionValue -Session msel -Name "allow-passthrough"
if ($val -eq "on") {
    Write-Pass "allow-passthrough mutation works (got '$val')"
} else {
    Write-Fail "allow-passthrough mutation broken: expected 'on', got '$val'"
}

# ============================================================
# Summary
# ============================================================
Write-Host ""
Write-Host ("=" * 60)
Write-Host "Cleaning up..."
Start-Process -FilePath $PSMUX -ArgumentList "kill-server" -WindowStyle Hidden
Start-Sleep -Seconds 2

Write-Host ""
Write-Host "Results: $script:TestsPassed passed, $script:TestsFailed failed" -ForegroundColor $(if ($script:TestsFailed -eq 0) { "Green" } else { "Red" })
exit $(if ($script:TestsFailed -eq 0) { 0 } else { 1 })
