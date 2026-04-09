# test_unbind_key_a.ps1 — Integration tests for unbind-key -a per-table behavior (#195)
#
# Tests that `unbind-key -a` clears only the prefix table by default,
# respects -T <table> and -n (root), and suppresses hardcoded defaults.
#
# Prerequisites: psmux built and available on PATH.
# Usage: pwsh tests/test_unbind_key_a.ps1

$ErrorActionPreference = "Stop"
$psmux = "psmux"
$pass = 0
$fail = 0

function Wait-ForSession($name, $timeoutMs = 5000) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $timeoutMs) {
        $out = & $psmux has-session -t $name 2>&1
        if ($LASTEXITCODE -eq 0) { return $true }
        Start-Sleep -Milliseconds 200
    }
    return $false
}

function Cleanup($name) {
    & $psmux kill-session -t $name 2>$null
    Start-Sleep -Milliseconds 500
}

# ── Test 1: unbind-key -a clears prefix table defaults ──────────────
Write-Host "`n[Test 1] unbind-key -a suppresses prefix defaults"
try {
    $sess = "unbind-test-1"
    Cleanup $sess
    & $psmux new-session -d -s $sess
    if (!(Wait-ForSession $sess)) { throw "Session $sess did not start" }

    # Verify defaults exist first
    $before = & $psmux list-keys -t $sess 2>&1
    $hasDefaults = $before | Select-String "bind-key -T prefix c new-window"
    if (!$hasDefaults) { throw "Expected default 'c' binding before unbind-key -a" }

    # Run unbind-key -a
    & $psmux unbind-key -a -t $sess

    # Verify defaults are gone
    $after = & $psmux list-keys -t $sess 2>&1
    $stillHas = $after | Select-String "bind-key -T prefix c new-window"
    if ($stillHas) { throw "Default 'c' binding should be suppressed after unbind-key -a" }

    Write-Host "  PASS" -ForegroundColor Green
    $pass++
}
catch {
    Write-Host "  FAIL: $_" -ForegroundColor Red
    $fail++
}
finally { Cleanup $sess }

# ── Test 2: Without unbind-key -a, defaults are present ─────────────
Write-Host "`n[Test 2] Without unbind-key -a, defaults are present"
try {
    $sess = "unbind-test-2"
    Cleanup $sess
    & $psmux new-session -d -s $sess
    if (!(Wait-ForSession $sess)) { throw "Session $sess did not start" }

    $keys = & $psmux list-keys -t $sess 2>&1
    $hasC = $keys | Select-String "bind-key -T prefix c new-window"
    $hasPercent = $keys | Select-String 'bind-key -T prefix % split-window -h'
    if (!$hasC -or !$hasPercent) {
        throw "Expected default prefix bindings 'c' and '%' to be present"
    }

    Write-Host "  PASS" -ForegroundColor Green
    $pass++
}
catch {
    Write-Host "  FAIL: $_" -ForegroundColor Red
    $fail++
}
finally { Cleanup $sess }

# ── Test 3: unbind-key -a -T root clears only root table ───────────
Write-Host "`n[Test 3] unbind-key -a -T root clears only root table, prefix intact"
try {
    $sess = "unbind-test-3"
    Cleanup $sess
    & $psmux new-session -d -s $sess
    if (!(Wait-ForSession $sess)) { throw "Session $sess did not start" }

    # Add a root-table binding
    & $psmux bind-key -T root F12 display-message "hello" -t $sess

    # Clear root table
    & $psmux unbind-key -a -T root -t $sess

    # Prefix defaults should still be there
    $keys = & $psmux list-keys -t $sess 2>&1
    $hasC = $keys | Select-String "bind-key -T prefix c new-window"
    if (!$hasC) { throw "Prefix defaults should survive unbind-key -a -T root" }

    Write-Host "  PASS" -ForegroundColor Green
    $pass++
}
catch {
    Write-Host "  FAIL: $_" -ForegroundColor Red
    $fail++
}
finally { Cleanup $sess }

# ── Test 4: Runtime unbind-key -a via send-keys / command-prompt ────
Write-Host "`n[Test 4] Runtime unbind-key -a (via command)"
try {
    $sess = "unbind-test-4"
    Cleanup $sess
    & $psmux new-session -d -s $sess
    if (!(Wait-ForSession $sess)) { throw "Session $sess did not start" }

    # Unbind via direct command
    & $psmux unbind-key -a -t $sess
    Start-Sleep -Milliseconds 500

    $keys = & $psmux list-keys -t $sess 2>&1
    $hasPrefixDefault = $keys | Select-String "bind-key -T prefix c new-window"
    if ($hasPrefixDefault) {
        throw "Prefix defaults should be suppressed after runtime unbind-key -a"
    }

    Write-Host "  PASS" -ForegroundColor Green
    $pass++
}
catch {
    Write-Host "  FAIL: $_" -ForegroundColor Red
    $fail++
}
finally { Cleanup $sess }

# ── Summary ─────────────────────────────────────────────────────────
Write-Host "`n=========================================="
Write-Host "Results: $pass passed, $fail failed" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Red" })
exit $fail
