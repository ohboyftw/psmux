# psmux Swarm Backend Validation Suite
# Run on Windows with psmux installed
# Usage: .\validate-swarm-backend.ps1

$ErrorActionPreference = "Continue"
$results = @()
$testCount = 0
$passCount = 0
$failCount = 0

# Allow nested session creation for testing purposes
$env:PSMUX_SESSION = $null
$env:TMUX = $null

function Test-Case {
    param([string]$Name, [scriptblock]$Test)
    $script:testCount++
    Write-Host "`n--- Test: $Name ---" -ForegroundColor Cyan
    try {
        $result = & $Test
        if ($result -eq $true) {
            $script:passCount++
            $script:results += @{ Name = $Name; Status = "PASS" }
            Write-Host "  PASS" -ForegroundColor Green
        } else {
            $script:failCount++
            $script:results += @{ Name = $Name; Status = "FAIL"; Detail = "Returned false" }
            Write-Host "  FAIL" -ForegroundColor Red
        }
    } catch {
        $script:failCount++
        $script:results += @{ Name = $Name; Status = "FAIL"; Detail = $_.Exception.Message }
        Write-Host "  FAIL: $($_.Exception.Message)" -ForegroundColor Red
    }
}

# Clean up any leftover test sessions
@("test-env", "test-panes", "test-split", "test-keys", "test-target",
  "test-concurrent", "test-persist", "test-format") | ForEach-Object {
    psmux kill-session -t $_ 2>$null
}

# ============================================================
# TEST 1: $TMUX environment variable
# ============================================================
Test-Case "TMUX env var is set in child shells" {
    psmux new-session -s test-env -d
    psmux send-keys -t test-env 'echo "TMUX_VAL=$env:TMUX"' Enter
    Start-Sleep -Seconds 2
    $captured = psmux capture-pane -t test-env -p
    psmux kill-session -t test-env
    [bool]($captured -match "TMUX_VAL=.+")
}

# ============================================================
# TEST 2: tmux binary resolves
# ============================================================
Test-Case "tmux command resolves to psmux" {
    $tmuxPath = Get-Command tmux -ErrorAction SilentlyContinue
    $null -ne $tmuxPath
}

# ============================================================
# TEST 3: list-panes output contains pane IDs
# ============================================================
Test-Case "list-panes returns %N pane identifiers" {
    psmux new-session -s test-panes -d
    psmux split-window -h -t test-panes
    psmux split-window -v -t test-panes
    $output = psmux list-panes -t test-panes
    psmux kill-session -t test-panes
    [bool]($output -match "%\d+")
}

# ============================================================
# TEST 4: list-panes marks active pane
# ============================================================
Test-Case "list-panes marks active pane" {
    psmux new-session -s test-panes -d
    psmux split-window -h -t test-panes
    $output = psmux list-panes -t test-panes
    psmux kill-session -t test-panes
    [bool]($output -match "\(active\)")
}

# ============================================================
# TEST 5: split-window -P -F returns pane ID
# ============================================================
Test-Case "split-window -P -F returns pane ID" {
    psmux new-session -s test-split -d
    $paneId = psmux split-window -h -t test-split -P -F "#{pane_id}"
    psmux kill-session -t test-split
    $paneId -match "^%\d+$"
}

# ============================================================
# TEST 6: send-keys delivers text to correct pane
# ============================================================
Test-Case "send-keys delivers text to targeted pane" {
    psmux new-session -s test-keys -d
    $pane = psmux split-window -h -t test-keys -P -F "#{pane_id}"
    psmux send-keys -t "test-keys:$pane" "echo VALIDATION_MARKER" Enter
    Start-Sleep -Seconds 2
    $captured = psmux capture-pane -t "test-keys:$pane" -p
    psmux kill-session -t test-keys
    [bool]($captured -match "VALIDATION_MARKER")
}

# ============================================================
# TEST 7: Pane targeting isolation
# ============================================================
Test-Case "send-keys to pane A does not leak to pane B" {
    psmux new-session -s test-target -d
    $paneA = psmux split-window -h -t test-target -P -F "#{pane_id}"
    $paneB = psmux split-window -v -t test-target -P -F "#{pane_id}"
    psmux send-keys -t "test-target:$paneA" "echo ONLY_A" Enter
    psmux send-keys -t "test-target:$paneB" "echo ONLY_B" Enter
    Start-Sleep -Seconds 2
    $outA = psmux capture-pane -t "test-target:$paneA" -p
    $outB = psmux capture-pane -t "test-target:$paneB" -p
    psmux kill-session -t test-target
    [bool]($outA -match "ONLY_A") -and -not [bool]($outA -match "ONLY_B") -and
    [bool]($outB -match "ONLY_B") -and -not [bool]($outB -match "ONLY_A")
}

# ============================================================
# TEST 8: Rapid concurrent splits
# ============================================================
Test-Case "5 rapid splits produce 5 unique pane IDs" {
    psmux new-session -s test-concurrent -d
    $panes = @()
    for ($i = 0; $i -lt 5; $i++) {
        # Alternate split direction to avoid running out of space
        $dir = if ($i % 2 -eq 0) { "-h" } else { "-v" }
        $id = psmux split-window $dir -t test-concurrent -P -F "#{pane_id}"
        if ($id) { $panes += $id }
    }
    psmux kill-session -t test-concurrent
    ($panes | Sort-Object -Unique).Count -eq 5
}

# ============================================================
# TEST 9: kill-pane removes only targeted pane
# ============================================================
Test-Case "kill-pane removes only targeted pane" {
    psmux new-session -s test-target -d
    $pane1 = psmux split-window -h -t test-target -P -F "#{pane_id}"
    $pane2 = psmux split-window -v -t test-target -P -F "#{pane_id}"
    psmux kill-pane -t "test-target:$pane1"
    $remaining = psmux list-panes -t test-target
    psmux kill-session -t test-target
    -not [bool]($remaining -match [regex]::Escape($pane1)) -and [bool]($remaining -match [regex]::Escape($pane2))
}

# ============================================================
# TEST 10: Session persistence after detach
# ============================================================
Test-Case "Session persists when detached" {
    psmux new-session -s test-persist -d
    psmux send-keys -t test-persist "echo PERSIST_CHECK" Enter
    Start-Sleep -Seconds 2
    # Session is already detached (-d flag). Verify it still exists.
    $exitCode = 0
    psmux has-session -t test-persist
    $exitCode = $LASTEXITCODE
    $captured = psmux capture-pane -t test-persist -p
    psmux kill-session -t test-persist
    ($exitCode -eq 0) -and [bool]($captured -match "PERSIST_CHECK")
}

# ============================================================
# TEST 11: Format string #{pane_pid}
# ============================================================
Test-Case "Format string #{pane_pid} returns numeric PID" {
    psmux new-session -s test-format -d
    $panePid = psmux split-window -h -t test-format -P -F "#{pane_pid}"
    psmux kill-session -t test-format
    $panePid -match "^\d+$"
}

# ============================================================
# TEST 12: send-keys -l literal mode
# ============================================================
Test-Case "send-keys -l sends text literally" {
    psmux new-session -s test-keys -d
    # "Enter" should NOT be parsed as a keypress in literal mode
    psmux send-keys -l -t test-keys "Enter is just text"
    Start-Sleep -Seconds 1
    $captured = psmux capture-pane -t test-keys -p
    psmux kill-session -t test-keys
    [bool]($captured -match "Enter is just text")
}

# ============================================================
# SUMMARY
# ============================================================
Write-Host "`n========================================" -ForegroundColor White
Write-Host "  SWARM BACKEND VALIDATION RESULTS" -ForegroundColor White
Write-Host "========================================" -ForegroundColor White
Write-Host "  Total:  $testCount" -ForegroundColor White
Write-Host "  Passed: $passCount" -ForegroundColor Green
Write-Host "  Failed: $failCount" -ForegroundColor $(if ($failCount -gt 0) { "Red" } else { "Green" })
Write-Host "========================================`n" -ForegroundColor White

if ($failCount -gt 0) {
    Write-Host "Failed tests:" -ForegroundColor Red
    $results | Where-Object { $_.Status -eq "FAIL" } | ForEach-Object {
        Write-Host "  - $($_.Name): $($_.Detail)" -ForegroundColor Red
    }
}

# Exit with failure code if any tests failed
exit $failCount
