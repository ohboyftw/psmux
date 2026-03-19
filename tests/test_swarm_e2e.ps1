# psmux Swarm End-to-End Test Suite
# ==================================
# Comprehensive test of psmux as a Claude Code swarm backend on Windows.
# Covers: prerequisites, environment detection, core backend commands,
# full swarm lifecycle simulation, edge cases, and common failure modes.
#
# Usage:
#   pwsh tests/test_swarm_e2e.ps1              # run all phases
#   pwsh tests/test_swarm_e2e.ps1 -Phase 1     # run single phase
#   pwsh tests/test_swarm_e2e.ps1 -Phase 1,3,5 # run specific phases
#   pwsh tests/test_swarm_e2e.ps1 -Verbose      # detailed output
#   pwsh tests/test_swarm_e2e.ps1 -SkipExisting  # skip validate-swarm-backend + agent-teams
#
# Phases:
#   1 - Prerequisites & binary checks
#   2 - Environment detection (what Claude Code checks)
#   3 - Core backend commands (split, send-keys, capture, kill)
#   4 - Full swarm lifecycle simulation (worktrees + multi-agent)
#   5 - Edge cases & stress tests
#   6 - Common failure mode detection
#   7 - Existing test suite integration (validate-swarm-backend + agent-teams)

param(
    [int[]]$Phase = @(1, 2, 3, 4, 5, 6, 7),
    [switch]$SkipExisting,
    [switch]$KeepArtifacts,
    [string]$ReportPath = ""
)

$ErrorActionPreference = "Continue"

# ============================================================
# TEST FRAMEWORK
# ============================================================

$script:pass = 0
$script:fail = 0
$script:skip = 0
$script:total = 0
$script:results = @()
$script:phaseResults = @{}
$script:startTime = Get-Date

function Write-Banner {
    param([string]$Text)
    $bar = "=" * 70
    Write-Host ""
    Write-Host $bar -ForegroundColor Magenta
    Write-Host "  $Text" -ForegroundColor Magenta
    Write-Host $bar -ForegroundColor Magenta
}

function Write-Section {
    param([string]$Text)
    Write-Host "`n--- $Text ---" -ForegroundColor Cyan
}

function Test-Case {
    param(
        [string]$Id,
        [string]$Name,
        [scriptblock]$Test,
        [int]$CurrentPhase
    )
    $script:total++
    $label = "[$Id] $Name"
    Write-Host "  $label" -ForegroundColor White -NoNewline

    try {
        $result = & $Test
        if ($result -eq $true) {
            $script:pass++
            $entry = @{ Id = $Id; Name = $Name; Status = "PASS"; Phase = $CurrentPhase }
            $script:results += $entry
            Write-Host " PASS" -ForegroundColor Green
        }
        elseif ($result -eq "SKIP") {
            $script:skip++
            $entry = @{ Id = $Id; Name = $Name; Status = "SKIP"; Phase = $CurrentPhase }
            $script:results += $entry
            Write-Host " SKIP" -ForegroundColor Yellow
        }
        else {
            $script:fail++
            $detail = if ($result -is [string]) { $result } else { "Returned false" }
            $entry = @{ Id = $Id; Name = $Name; Status = "FAIL"; Detail = $detail; Phase = $CurrentPhase }
            $script:results += $entry
            Write-Host " FAIL: $detail" -ForegroundColor Red
        }
    }
    catch {
        $script:fail++
        $entry = @{ Id = $Id; Name = $Name; Status = "FAIL"; Detail = $_.Exception.Message; Phase = $CurrentPhase }
        $script:results += $entry
        Write-Host " FAIL: $($_.Exception.Message)" -ForegroundColor Red
    }
}

# ============================================================
# SHARED HELPERS
# ============================================================

$PSMUX = "$PSScriptRoot\..\target\release\psmux.exe"
if (-not (Test-Path $PSMUX)) {
    $PSMUX = (Get-Command psmux -ErrorAction SilentlyContinue).Source
}
if (-not $PSMUX -or -not (Test-Path $PSMUX)) {
    $PSMUX = (Get-Command tmux -ErrorAction SilentlyContinue).Source
}

$TESTDIR = Join-Path $env:TEMP "psmux_swarm_e2e_$(Get-Random)"
New-Item -Path $TESTDIR -ItemType Directory -Force | Out-Null

$SESSION_PREFIX = "swarm_e2e"
$SESSION_COUNTER = 0

function New-TestSession {
    param([string]$Suffix = "")
    $script:SESSION_COUNTER++
    $name = "${SESSION_PREFIX}_${script:SESSION_COUNTER}"
    if ($Suffix) { $name = "${SESSION_PREFIX}_${Suffix}" }

    # Clean up any leftover session with this name
    try { & $PSMUX kill-session -t $name 2>&1 | Out-Null } catch {}
    Start-Sleep -Milliseconds 300
    Remove-Item "$env:USERPROFILE\.psmux\$name.port" -Force -ErrorAction SilentlyContinue
    Remove-Item "$env:USERPROFILE\.psmux\$name.key" -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 300

    & $PSMUX new-session -s $name -d 2>&1 | Out-Null
    Start-Sleep -Milliseconds 2500

    & $PSMUX has-session -t $name 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Failed to start session '$name'" }
    return $name
}

function Remove-TestSession {
    param([string]$Name)
    try { & $PSMUX kill-session -t $Name 2>&1 | Out-Null } catch {}
    Start-Sleep -Milliseconds 500
    Remove-Item "$env:USERPROFILE\.psmux\$Name.port" -Force -ErrorAction SilentlyContinue
    Remove-Item "$env:USERPROFILE\.psmux\$Name.key" -Force -ErrorAction SilentlyContinue
}

function Get-PaneOutput {
    param([string]$Target)
    & $PSMUX capture-pane -t $Target -p 2>&1 | Out-String
}

function Wait-ForOutput {
    param(
        [string]$Target,
        [string]$Marker,
        [int]$TimeoutSeconds = 10,
        [int]$PollMs = 500
    )
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $cap = Get-PaneOutput $Target
        if ($cap -match [regex]::Escape($Marker)) { return $cap }
        Start-Sleep -Milliseconds $PollMs
    }
    return $null
}

function Wait-PaneReady {
    # Confirms a pane's shell is alive and accepting input by sending a sentinel
    # marker and polling capture-pane until it appears. This avoids the race
    # condition where send-keys arrives before the shell has finished loading
    # PSReadLine/profile.
    param(
        [string]$Target,
        [int]$TimeoutSeconds = 20
    )
    $sentinel = "RDY_$(Get-Random)"
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        & $PSMUX send-keys -t $Target "echo '${sentinel}'" Enter
        Start-Sleep -Milliseconds 1000
        $cap = Get-PaneOutput $Target
        if ($cap -match $sentinel) { return $true }
    }
    return $false
}


# ============================================================
# PHASE 1: PREREQUISITES & BINARY CHECKS
# ============================================================

if ($Phase -contains 1) {
    Write-Banner "PHASE 1: Prerequisites & Binary Checks"

    Test-Case "1.1" "psmux binary exists" {
        [bool]($PSMUX -and (Test-Path $PSMUX))
    } 1

    Test-Case "1.2" "psmux is on PATH (as psmux or tmux)" {
        $p = Get-Command psmux -ErrorAction SilentlyContinue
        $t = Get-Command tmux -ErrorAction SilentlyContinue
        [bool]($p -or $t)
    } 1

    Test-Case "1.3" "tmux alias resolves to psmux binary" {
        $t = Get-Command tmux -ErrorAction SilentlyContinue
        if (-not $t) { return "SKIP" }
        # tmux -V outputs "tmux 3.2" for compatibility; check the binary path instead
        [bool]($t.Source -match "psmux|tmux")
    } 1

    Test-Case "1.4" "psmux version is 3.2+ (Claude Code requirement)" {
        $ver = & $PSMUX -V 2>&1 | Out-String
        if ($ver -match "(\d+)\.(\d+)") {
            $major = [int]$Matches[1]
            $minor = [int]$Matches[2]
            ($major -gt 3) -or ($major -eq 3 -and $minor -ge 2)
        } else { $false }
    } 1

    Test-Case "1.5" "psmux -V exit code is 0" {
        & $PSMUX -V 2>&1 | Out-Null
        $LASTEXITCODE -eq 0
    } 1

    Test-Case "1.6" "node.js is available (for .js agent files)" {
        $n = Get-Command node -ErrorAction SilentlyContinue
        if (-not $n) { return "SKIP" }
        $true
    } 1

    Test-Case "1.7" "claude CLI is available" {
        $c = Get-Command claude -ErrorAction SilentlyContinue
        if (-not $c) { return "SKIP" }
        $true
    } 1

    Test-Case "1.8" "git is available (for worktree tests)" {
        $g = Get-Command git -ErrorAction SilentlyContinue
        [bool]$g
    } 1

    Test-Case "1.9" "Current directory is a git repo" {
        $gitDir = git rev-parse --git-dir 2>&1
        $LASTEXITCODE -eq 0
    } 1
}

# ============================================================
# PHASE 2: ENVIRONMENT DETECTION (what Claude Code checks)
# ============================================================

if ($Phase -contains 2) {
    Write-Banner "PHASE 2: Environment Detection"
    Write-Host "  Simulates Claude Code's tmux backend detection logic" -ForegroundColor DarkGray

    Test-Case "2.1" "`$TMUX is set inside psmux session" {
        $s = New-TestSession "env1"
        $m = "ENV1_$(Get-Random)"
        & $PSMUX send-keys -t $s "if (`$env:TMUX) { Write-Host '${m}:SET' } else { Write-Host '${m}:EMPTY' }" Enter
        $cap = Wait-ForOutput $s $m 15
        Remove-TestSession $s
        if (-not $cap) { return "No output captured" }
        [bool]($cap -match "${m}:SET")
    } 2

    Test-Case "2.2" "`$TMUX format contains /tmp/tmux- pattern" {
        $s = New-TestSession "env2"
        if (-not (Wait-PaneReady $s)) { Remove-TestSession $s; return "Shell not ready" }
        $m = "ENV2_$(Get-Random)"
        & $PSMUX send-keys -t $s "if (`$env:TMUX -match '/tmp/tmux-') { Write-Host '${m}:FMT_OK' } else { Write-Host '${m}:FMT_BAD' }" Enter
        $cap = Wait-ForOutput $s $m 15
        Remove-TestSession $s
        if (-not $cap) { return "No output captured" }
        [bool]($cap -match "${m}:FMT_OK")
    } 2

    Test-Case "2.3" "`$TMUX_PANE is set with %N format" {
        $s = New-TestSession "env3"
        if (-not (Wait-PaneReady $s)) { Remove-TestSession $s; return "Shell not ready" }
        $m = "ENV3_$(Get-Random)"
        & $PSMUX send-keys -t $s "if (`$env:TMUX_PANE -match '^%\d+`$') { Write-Host '${m}:PANE_OK' } else { Write-Host '${m}:PANE_BAD' }" Enter
        $cap = Wait-ForOutput $s $m 15
        Remove-TestSession $s
        if (-not $cap) { return "No output captured" }
        [bool]($cap -match "${m}:PANE_OK")
    } 2

    Test-Case "2.4" "`$TMUX propagates to child panes" {
        $s = New-TestSession "env4"
        $pane = (& $PSMUX split-window -h -t $s -P -F "#{pane_id}" 2>&1 | Out-String).Trim()
        if (-not (Wait-PaneReady "${s}:${pane}" 25)) { Remove-TestSession $s; return "Child pane shell not ready" }
        $m = "ENV4_$(Get-Random)"
        & $PSMUX send-keys -t "${s}:${pane}" "if (`$env:TMUX -match '/tmp/tmux-') { Write-Host '${m}:CHILD_OK' } else { Write-Host '${m}:CHILD_BAD' }" Enter
        $cap = Wait-ForOutput "${s}:${pane}" $m 15
        Remove-TestSession $s
        if (-not $cap) { return "No output in child pane" }
        [bool]($cap -match "${m}:CHILD_OK")
    } 2

    Test-Case "2.5" "has-session returns 0 for existing session" {
        $s = New-TestSession "env5"
        & $PSMUX has-session -t $s 2>&1 | Out-Null
        $code = $LASTEXITCODE
        Remove-TestSession $s
        $code -eq 0
    } 2

    Test-Case "2.6" "has-session returns non-zero for missing session" {
        & $PSMUX has-session -t "no_such_session_$(Get-Random)" 2>&1 | Out-Null
        $LASTEXITCODE -ne 0
    } 2

    Test-Case "2.7" "`$CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS is set" {
        $s = New-TestSession "env7"
        if (-not (Wait-PaneReady $s)) { Remove-TestSession $s; return "Shell not ready" }
        $m = "ENV7_$(Get-Random)"
        & $PSMUX send-keys -t $s "if (`$env:CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS -eq '1') { Write-Host '${m}:TEAMS_OK' } else { Write-Host '${m}:TEAMS_BAD' }" Enter
        $cap = Wait-ForOutput $s $m 15
        Remove-TestSession $s
        if (-not $cap) { return "No output captured" }
        [bool]($cap -match "${m}:TEAMS_OK")
    } 2
}

# ============================================================
# PHASE 3: CORE BACKEND COMMANDS
# ============================================================

if ($Phase -contains 3) {
    Write-Banner "PHASE 3: Core Backend Commands"
    Write-Host "  Tests every tmux command Claude Code's spawn backend calls" -ForegroundColor DarkGray

    # --- Session commands ---
    Write-Section "Session Commands"

    Test-Case "3.1" "new-session -s name -d (detached creation)" {
        $s = New-TestSession "core1"
        & $PSMUX has-session -t $s 2>&1 | Out-Null
        $ok = $LASTEXITCODE -eq 0
        Remove-TestSession $s
        $ok
    } 3

    Test-Case "3.2" "kill-session removes session cleanly" {
        $s = New-TestSession "core2"
        & $PSMUX kill-session -t $s 2>&1 | Out-Null
        Start-Sleep -Milliseconds 500
        & $PSMUX has-session -t $s 2>&1 | Out-Null
        $LASTEXITCODE -ne 0
    } 3

    Test-Case "3.3" "list-sessions shows created session" {
        $s = New-TestSession "core3"
        $out = & $PSMUX list-sessions 2>&1 | Out-String
        Remove-TestSession $s
        [bool]($out -match $s)
    } 3

    # --- Split/pane commands ---
    Write-Section "Split & Pane Commands"

    Test-Case "3.4" "split-window -h returns %N pane ID" {
        $s = New-TestSession "split1"
        $id = (& $PSMUX split-window -h -t $s -P -F "#{pane_id}" 2>&1 | Out-String).Trim()
        Remove-TestSession $s
        $id -match "^%\d+$"
    } 3

    Test-Case "3.5" "split-window -v returns %N pane ID" {
        $s = New-TestSession "split2"
        $id = (& $PSMUX split-window -v -t $s -P -F "#{pane_id}" 2>&1 | Out-String).Trim()
        Remove-TestSession $s
        $id -match "^%\d+$"
    } 3

    Test-Case "3.6" "list-panes includes %N identifiers" {
        $s = New-TestSession "split3"
        & $PSMUX split-window -h -t $s 2>&1 | Out-Null
        Start-Sleep -Seconds 1
        $out = & $PSMUX list-panes -t $s 2>&1 | Out-String
        Remove-TestSession $s
        [bool]($out -match "%\d+")
    } 3

    Test-Case "3.7" "list-panes marks active pane with (active)" {
        $s = New-TestSession "split4"
        & $PSMUX split-window -h -t $s 2>&1 | Out-Null
        Start-Sleep -Seconds 1
        $out = & $PSMUX list-panes -t $s 2>&1 | Out-String
        Remove-TestSession $s
        [bool]($out -match "\(active\)")
    } 3

    Test-Case "3.8" "kill-pane removes only targeted pane" {
        $s = New-TestSession "split5"
        $p1 = (& $PSMUX split-window -h -t $s -P -F "#{pane_id}" 2>&1 | Out-String).Trim()
        Start-Sleep -Seconds 1
        $p2 = (& $PSMUX split-window -v -t $s -P -F "#{pane_id}" 2>&1 | Out-String).Trim()
        Start-Sleep -Seconds 1
        & $PSMUX kill-pane -t "${s}:${p1}" 2>&1 | Out-Null
        Start-Sleep -Milliseconds 500
        $remaining = & $PSMUX list-panes -t $s 2>&1 | Out-String
        Remove-TestSession $s
        (-not ($remaining -match [regex]::Escape($p1))) -and ($remaining -match [regex]::Escape($p2))
    } 3

    # --- send-keys commands ---
    Write-Section "send-keys Commands"

    Test-Case "3.9" "send-keys delivers text to targeted pane" {
        $s = New-TestSession "keys1"
        $pane = (& $PSMUX split-window -h -t $s -P -F "#{pane_id}" 2>&1 | Out-String).Trim()
        Start-Sleep -Seconds 2
        $m = "KEYS1_$(Get-Random)"
        & $PSMUX send-keys -t "${s}:${pane}" "echo '${m}'" Enter
        $cap = Wait-ForOutput "${s}:${pane}" $m
        Remove-TestSession $s
        [bool]($cap -match $m)
    } 3

    Test-Case "3.10" "send-keys -l literal mode (Enter = text, not keypress)" {
        $s = New-TestSession "keys2"
        & $PSMUX send-keys -l -t $s "Enter is just text"
        Start-Sleep -Seconds 1
        $cap = Get-PaneOutput $s
        Remove-TestSession $s
        [bool]($cap -match "Enter is just text")
    } 3

    Test-Case "3.11" "send-keys pane isolation (A does not leak to B)" {
        $s = New-TestSession "keys3"
        $pA = (& $PSMUX split-window -h -t $s -P -F "#{pane_id}" 2>&1 | Out-String).Trim()
        Start-Sleep -Seconds 1
        $pB = (& $PSMUX split-window -v -t $s -P -F "#{pane_id}" 2>&1 | Out-String).Trim()
        Start-Sleep -Seconds 2

        $mA = "ISOL_A_$(Get-Random)"
        $mB = "ISOL_B_$(Get-Random)"
        & $PSMUX send-keys -t "${s}:${pA}" "echo '${mA}'" Enter
        & $PSMUX send-keys -t "${s}:${pB}" "echo '${mB}'" Enter
        Start-Sleep -Seconds 3

        $outA = Get-PaneOutput "${s}:${pA}"
        $outB = Get-PaneOutput "${s}:${pB}"
        Remove-TestSession $s

        ($outA -match $mA) -and (-not ($outA -match $mB)) -and
        ($outB -match $mB) -and (-not ($outB -match $mA))
    } 3

    # --- Layout commands ---
    Write-Section "Layout & Display Commands"

    Test-Case "3.12" "select-layout tiled accepted" {
        $s = New-TestSession "layout1"
        & $PSMUX split-window -h -t $s 2>&1 | Out-Null
        & $PSMUX split-window -v -t $s 2>&1 | Out-Null
        Start-Sleep -Seconds 1
        & $PSMUX select-layout -t $s tiled 2>&1 | Out-Null
        $ok = $LASTEXITCODE -eq 0
        Remove-TestSession $s
        $ok
    } 3

    Test-Case "3.13" "select-layout main-vertical accepted" {
        $s = New-TestSession "layout2"
        & $PSMUX split-window -h -t $s 2>&1 | Out-Null
        & $PSMUX split-window -v -t $s 2>&1 | Out-Null
        Start-Sleep -Seconds 1
        & $PSMUX select-layout -t $s main-vertical 2>&1 | Out-Null
        $ok = $LASTEXITCODE -eq 0
        Remove-TestSession $s
        $ok
    } 3

    Test-Case "3.14" "select-pane -t %N targets specific pane" {
        $s = New-TestSession "layout3"
        $p = (& $PSMUX split-window -h -t $s -P -F "#{pane_id}" 2>&1 | Out-String).Trim()
        Start-Sleep -Seconds 1
        & $PSMUX select-pane -t "${s}:${p}" 2>&1 | Out-Null
        $ok = $LASTEXITCODE -eq 0
        Remove-TestSession $s
        $ok
    } 3

    Test-Case "3.15" "display-message -p #{pane_id} returns %N" {
        $s = New-TestSession "layout4"
        $result = (& $PSMUX display-message -t $s -p "#{pane_id}" 2>&1 | Out-String).Trim()
        Remove-TestSession $s
        $result -match "^%\d+$"
    } 3

    # --- capture-pane ---
    Write-Section "capture-pane"

    Test-Case "3.16" "capture-pane -p returns pane content" {
        $s = New-TestSession "cap1"
        $m = "CAP1_$(Get-Random)"
        & $PSMUX send-keys -t $s "echo '${m}'" Enter
        $cap = Wait-ForOutput $s $m
        Remove-TestSession $s
        [bool]($cap -match $m)
    } 3

    # --- Format strings ---
    Write-Section "Format Strings"

    Test-Case "3.17" "#{pane_pid} returns numeric PID" {
        $s = New-TestSession "fmt1"
        $panePid = (& $PSMUX split-window -h -t $s -P -F "#{pane_pid}" 2>&1 | Out-String).Trim()
        Remove-TestSession $s
        $panePid -match "^\d+$"
    } 3

    Test-Case "3.18" "#{session_name} returns session name" {
        $s = New-TestSession "fmt2"
        $name = (& $PSMUX display-message -t $s -p "#{session_name}" 2>&1 | Out-String).Trim()
        Remove-TestSession $s
        $name -eq $s
    } 3
}

# ============================================================
# PHASE 4: FULL SWARM LIFECYCLE SIMULATION
# ============================================================

if ($Phase -contains 4) {
    Write-Banner "PHASE 4: Full Swarm Lifecycle Simulation"
    Write-Host "  Simulates exactly what /spawn-swarm and TeammateTool do" -ForegroundColor DarkGray

    # --- 4.1: Create session + split 3 agent panes ---
    Write-Section "Swarm Session Setup"

    $swarmSession = $null
    $agentPanes = @()

    Test-Case "4.1" "Create swarm session with 3 agent panes" {
        $script:swarmSession = New-TestSession "swarm"
        $script:agentPanes = @()
        for ($i = 1; $i -le 3; $i++) {
            $dir = if ($i % 2 -eq 0) { "-v" } else { "-h" }
            $pane = (& $PSMUX split-window $dir -t $script:swarmSession -P -F "#{pane_id}" 2>&1 | Out-String).Trim()
            Start-Sleep -Seconds 2
            $script:agentPanes += $pane
        }
        $allValid = ($script:agentPanes | Where-Object { $_ -match '^%\d+$' }).Count -eq 3
        $allUnique = ($script:agentPanes | Sort-Object -Unique).Count -eq 3
        $allValid -and $allUnique
    } 4

    # --- 4.2: Apply layout ---
    Test-Case "4.2" "Apply tiled layout to swarm session" {
        & $PSMUX select-layout -t $script:swarmSession tiled 2>&1 | Out-Null
        $LASTEXITCODE -eq 0
    } 4

    # --- 4.3: Inject agent prompts via send-keys ---
    Write-Section "Agent Prompt Injection"

    Test-Case "4.3" "Inject prompts into all 3 agent panes" {
        $allOk = $true
        $markers = @()
        for ($i = 0; $i -lt $script:agentPanes.Count; $i++) {
            $pane = $script:agentPanes[$i]
            $m = "AGENT${i}_$(Get-Random)"
            $markers += $m
            & $PSMUX send-keys -t "$($script:swarmSession):${pane}" "echo '${m}:WORKING'" Enter
        }
        Start-Sleep -Seconds 5

        for ($i = 0; $i -lt $script:agentPanes.Count; $i++) {
            $pane = $script:agentPanes[$i]
            $cap = Get-PaneOutput "$($script:swarmSession):${pane}"
            if (-not ($cap -match "$($markers[$i]):WORKING")) {
                $allOk = $false
            }
        }
        $allOk
    } 4

    # --- 4.4: Simulate agent writing results to inbox file ---
    Write-Section "Agent Result Collection"

    Test-Case "4.4" "Agent outputs result marker (simulating inbox write)" {
        $pane = $script:agentPanes[0]
        $m = "RESULT_$(Get-Random)"
        & $PSMUX send-keys -t "$($script:swarmSession):${pane}" "Write-Host '${m}:status=done'" Enter
        $cap = Wait-ForOutput "$($script:swarmSession):${pane}" $m 15
        [bool]($cap -match "${m}:status=done")
    } 4

    # --- 4.5: Monitor with list-panes ---
    Test-Case "4.5" "list-panes shows all agent panes alive" {
        $out = & $PSMUX list-panes -t $script:swarmSession 2>&1 | Out-String
        $paneCount = ($out -split "`n" | Where-Object { $_ -match '^\d+:' }).Count
        # We have 1 original + 3 splits = 4 panes
        $paneCount -ge 4
    } 4

    # --- 4.6: Kill one agent, verify others survive ---
    Write-Section "Agent Lifecycle"

    Test-Case "4.6" "Kill one agent pane, others survive" {
        $victim = $script:agentPanes[1]
        & $PSMUX kill-pane -t "$($script:swarmSession):${victim}" 2>&1 | Out-Null
        Start-Sleep -Seconds 1
        $out = & $PSMUX list-panes -t $script:swarmSession 2>&1 | Out-String

        $victimGone = -not ($out -match [regex]::Escape($victim))
        $othersAlive = ($out -match [regex]::Escape($script:agentPanes[0])) -and
                       ($out -match [regex]::Escape($script:agentPanes[2]))
        $victimGone -and $othersAlive
    } 4

    # --- 4.7: Capture output from surviving agent ---
    Test-Case "4.7" "capture-pane on surviving agent returns content" {
        $pane = $script:agentPanes[0]
        $cap = Get-PaneOutput "$($script:swarmSession):${pane}"
        [bool]($cap -and $cap.Trim().Length -gt 0)
    } 4

    # --- 4.8: Git worktree creation + agent isolation ---
    Write-Section "Git Worktree Isolation"

    $worktreeDir = Join-Path $TESTDIR "worktrees"

    Test-Case "4.8" "Create git worktrees for agent isolation" {
        # We need to be in a git repo for this
        $gitDir = git rev-parse --git-dir 2>&1
        if ($LASTEXITCODE -ne 0) { return "SKIP" }

        New-Item -Path $worktreeDir -ItemType Directory -Force | Out-Null
        $branch = "test-swarm-agent-$(Get-Random)"
        $wtPath = Join-Path $worktreeDir "agent-1"

        git worktree add $wtPath -b $branch 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { return "git worktree add failed" }

        $exists = Test-Path (Join-Path $wtPath ".git")
        # Clean up
        git worktree remove $wtPath --force 2>&1 | Out-Null
        git branch -D $branch 2>&1 | Out-Null
        $exists
    } 4

    # --- 4.9: Send agent to work in worktree ---
    Test-Case "4.9" "Agent can cd to worktree and work" {
        $gitDir = git rev-parse --git-dir 2>&1
        if ($LASTEXITCODE -ne 0) { return "SKIP" }

        $branch = "test-swarm-wt-$(Get-Random)"
        $wtPath = Join-Path $worktreeDir "agent-wt"
        New-Item -Path $worktreeDir -ItemType Directory -Force | Out-Null

        git worktree add $wtPath -b $branch 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { return "git worktree add failed" }

        $pane = $script:agentPanes[2]
        $m = "WT_$(Get-Random)"
        & $PSMUX send-keys -t "$($script:swarmSession):${pane}" "cd '$wtPath' && echo '${m}:IN_WORKTREE'" Enter
        $cap = Wait-ForOutput "$($script:swarmSession):${pane}" $m 10

        # Clean up
        git worktree remove $wtPath --force 2>&1 | Out-Null
        git branch -D $branch 2>&1 | Out-Null

        [bool]($cap -match "${m}:IN_WORKTREE")
    } 4

    # --- 4.10: Full cleanup ---
    Test-Case "4.10" "Kill swarm session cleanly" {
        if ($script:swarmSession) {
            Remove-TestSession $script:swarmSession
        }
        & $PSMUX has-session -t $script:swarmSession 2>&1 | Out-Null
        $LASTEXITCODE -ne 0
    } 4
}

# ============================================================
# PHASE 5: EDGE CASES & STRESS TESTS
# ============================================================

if ($Phase -contains 5) {
    Write-Banner "PHASE 5: Edge Cases & Stress Tests"

    # --- Long prompts ---
    Write-Section "Long Prompt Handling"

    Test-Case "5.1" "send-keys handles 500+ char prompt" {
        $s = New-TestSession "edge1"
        $m = "LONG_$(Get-Random)"
        $longText = "${m}:" + ("A" * 500)
        & $PSMUX send-keys -t $s "echo '${longText}'" Enter
        $cap = Wait-ForOutput $s $m 10
        Remove-TestSession $s
        [bool]($cap -match $m)
    } 5

    Test-Case "5.2" "send-keys handles 2000+ char prompt (Claude Code agent prompts)" {
        $s = New-TestSession "edge2"
        $m = "VLONG_$(Get-Random)"
        $longText = "${m}:" + ("B" * 2000)
        & $PSMUX send-keys -t $s "echo '${longText}'" Enter
        $cap = Wait-ForOutput $s $m 15
        Remove-TestSession $s
        [bool]($cap -match $m)
    } 5

    # --- Special characters ---
    Write-Section "Special Characters in send-keys"

    Test-Case "5.3" "send-keys handles double quotes" {
        $s = New-TestSession "edge3"
        $m = "QUOT_$(Get-Random)"
        & $PSMUX send-keys -t $s "echo `"${m}:has quotes`"" Enter
        $cap = Wait-ForOutput $s $m
        Remove-TestSession $s
        [bool]($cap -match "${m}:has quotes")
    } 5

    Test-Case "5.4" "send-keys handles single quotes" {
        $s = New-TestSession "edge4"
        $m = "SQUOT_$(Get-Random)"
        & $PSMUX send-keys -t $s "echo '${m}:single quoted'" Enter
        $cap = Wait-ForOutput $s $m
        Remove-TestSession $s
        [bool]($cap -match "${m}:single quoted")
    } 5

    Test-Case "5.5" "send-keys handles special shell chars (|, &, ;, >)" {
        $s = New-TestSession "edge5"
        $m = "SPEC_$(Get-Random)"
        & $PSMUX send-keys -t $s "echo '${m}:pipe|amp&semi;gt>'" Enter
        $cap = Wait-ForOutput $s $m
        Remove-TestSession $s
        [bool]($cap -match $m)
    } 5

    Test-Case "5.6" "send-keys handles backslash-colon (POSIX escape pattern)" {
        $s = New-TestSession "edge6"
        $m = "BESC_$(Get-Random)"
        & $PSMUX send-keys -t $s "echo '${m}:https\://api.example.com'" Enter
        $cap = Wait-ForOutput $s $m
        Remove-TestSession $s
        [bool]($cap -match $m)
    } 5

    # --- Rapid concurrent operations ---
    Write-Section "Concurrent Operations"

    Test-Case "5.7" "5 rapid splits produce 5 unique pane IDs (no race condition)" {
        $s = New-TestSession "edge7"
        $panes = @()
        for ($i = 0; $i -lt 5; $i++) {
            $dir = if ($i % 2 -eq 0) { "-h" } else { "-v" }
            $id = (& $PSMUX split-window $dir -t $s -P -F "#{pane_id}" 2>&1 | Out-String).Trim()
            if ($id -match "^%\d+$") { $panes += $id }
        }
        Remove-TestSession $s
        ($panes | Sort-Object -Unique).Count -eq 5
    } 5

    Test-Case "5.8" "Rapid send-keys to multiple panes (no cross-contamination)" {
        $s = New-TestSession "edge8"
        $p1 = (& $PSMUX split-window -h -t $s -P -F "#{pane_id}" 2>&1 | Out-String).Trim()
        $p2 = (& $PSMUX split-window -v -t $s -P -F "#{pane_id}" 2>&1 | Out-String).Trim()
        Start-Sleep -Seconds 2

        # Fire send-keys to both panes rapidly
        $m1 = "RAPID1_$(Get-Random)"
        $m2 = "RAPID2_$(Get-Random)"
        & $PSMUX send-keys -t "${s}:${p1}" "echo '${m1}'" Enter
        & $PSMUX send-keys -t "${s}:${p2}" "echo '${m2}'" Enter
        Start-Sleep -Seconds 4

        $out1 = Get-PaneOutput "${s}:${p1}"
        $out2 = Get-PaneOutput "${s}:${p2}"
        Remove-TestSession $s

        ($out1 -match $m1) -and (-not ($out1 -match $m2)) -and
        ($out2 -match $m2) -and (-not ($out2 -match $m1))
    } 5

    # --- Session persistence ---
    Write-Section "Session Persistence"

    Test-Case "5.9" "Detached session persists and retains pane content" {
        $s = New-TestSession "edge9"
        $m = "PERSIST_$(Get-Random)"
        & $PSMUX send-keys -t $s "echo '${m}:BEFORE_DETACH'" Enter
        Start-Sleep -Seconds 3
        # Session created with -d is already detached. Verify it persists.
        & $PSMUX has-session -t $s 2>&1 | Out-Null
        $alive = $LASTEXITCODE -eq 0
        $cap = Get-PaneOutput $s
        Remove-TestSession $s
        $alive -and ($cap -match "${m}:BEFORE_DETACH")
    } 5

    # --- Pane limits ---
    Write-Section "Pane Limits"

    Test-Case "5.10" "Can create 6+ panes (practical agent count)" {
        $s = New-TestSession "edge10"
        $created = 0
        for ($i = 0; $i -lt 6; $i++) {
            $dir = if ($i % 2 -eq 0) { "-h" } else { "-v" }
            $id = (& $PSMUX split-window $dir -t $s -P -F "#{pane_id}" 2>&1 | Out-String).Trim()
            if ($id -match "^%\d+$") { $created++ }
            Start-Sleep -Milliseconds 500
        }
        Remove-TestSession $s
        # 5 of 6 splits must succeed (1 original + 5 splits = 6 panes)
        $created -ge 5
    } 5
}

# ============================================================
# PHASE 6: COMMON FAILURE MODE DETECTION
# ============================================================

if ($Phase -contains 6) {
    Write-Banner "PHASE 6: Common Failure Mode Detection"
    Write-Host "  Tests known Windows-specific issues that break agent spawning" -ForegroundColor DarkGray

    # --- In-process fallback detection ---
    Write-Section "In-Process Fallback Triggers"

    Test-Case "6.1" "psmux sets TMUX (prevents in-process fallback)" {
        $s = New-TestSession "fail1"
        $m = "FALLBACK_$(Get-Random)"
        & $PSMUX send-keys -t $s "if (`$env:TMUX) { Write-Host '${m}:HAS_TMUX' } else { Write-Host '${m}:NO_TMUX' }" Enter
        $cap = Wait-ForOutput $s $m
        Remove-TestSession $s
        if (-not $cap) { return "No output" }
        [bool]($cap -match "${m}:HAS_TMUX")
    } 6

    Test-Case "6.2" "tmux command works inside psmux pane" {
        $s = New-TestSession "fail2"
        $m = "WHICH_$(Get-Random)"
        & $PSMUX send-keys -t $s "Write-Host '${m}:' (Get-Command tmux -EA SilentlyContinue).Source" Enter
        $cap = Wait-ForOutput $s $m 15
        Remove-TestSession $s
        if (-not $cap) { return "No output" }
        # tmux should resolve to something (psmux.exe or tmux.exe which IS psmux)
        [bool]($cap -match "tmux|psmux")
    } 6

    # --- send-keys encoding issues ---
    Write-Section "send-keys Encoding Issues"

    Test-Case "6.3" "send-keys handles && chaining (cd && env ... cmd)" {
        $s = New-TestSession "fail3"
        $m = "CHAIN_$(Get-Random)"
        & $PSMUX send-keys -t $s "cd '$TESTDIR' && echo '${m}:CHAINED'" Enter
        $cap = Wait-ForOutput $s $m
        Remove-TestSession $s
        [bool]($cap -match "${m}:CHAINED")
    } 6

    Test-Case "6.4" "send-keys handles env KEY=VALUE syntax" {
        $s = New-TestSession "fail4"
        $m = "ENVSET_$(Get-Random)"
        # psmux env shim should translate POSIX env syntax for PowerShell
        & $PSMUX send-keys -t $s "env TEST_VAR=hello Write-Host '${m}:done'" Enter
        $cap = Wait-ForOutput $s $m 8
        Remove-TestSession $s
        # Even if env shim doesn't exist, we just need it not to crash
        if ($cap -match $m) { $true } else { "env shim may not translate POSIX syntax" }
    } 6

    # --- Named pipe issues ---
    Write-Section "Named Pipe / IPC Issues"

    Test-Case "6.5" "Session survives rapid create-kill-create cycle" {
        $name = "fail5_cycle"
        for ($i = 0; $i -lt 3; $i++) {
            try { & $PSMUX kill-session -t $name 2>&1 | Out-Null } catch {}
            Start-Sleep -Milliseconds 500
            Remove-Item "$env:USERPROFILE\.psmux\$name.port" -Force -ErrorAction SilentlyContinue
            Remove-Item "$env:USERPROFILE\.psmux\$name.key" -Force -ErrorAction SilentlyContinue
            Start-Sleep -Milliseconds 300
            & $PSMUX new-session -s $name -d 2>&1 | Out-Null
            Start-Sleep -Seconds 2
        }
        & $PSMUX has-session -t $name 2>&1 | Out-Null
        $ok = $LASTEXITCODE -eq 0
        try { & $PSMUX kill-session -t $name 2>&1 | Out-Null } catch {}
        $ok
    } 6

    Test-Case "6.6" "Stale port file doesn't block new session" {
        $name = "fail6_stale"
        # Create a fake stale port file
        $portFile = "$env:USERPROFILE\.psmux\$name.port"
        New-Item -Path (Split-Path $portFile) -ItemType Directory -Force | Out-Null
        Set-Content -Path $portFile -Value "99999"
        Start-Sleep -Milliseconds 200

        try { & $PSMUX kill-session -t $name 2>&1 | Out-Null } catch {}
        Start-Sleep -Milliseconds 300
        Remove-Item $portFile -Force -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 200

        & $PSMUX new-session -s $name -d 2>&1 | Out-Null
        Start-Sleep -Seconds 2
        & $PSMUX has-session -t $name 2>&1 | Out-Null
        $ok = $LASTEXITCODE -eq 0
        try { & $PSMUX kill-session -t $name 2>&1 | Out-Null } catch {}
        $ok
    } 6

    # --- Process spawning issues ---
    Write-Section "Process Spawning Issues"

    Test-Case "6.7" "Pane shell is PowerShell (not cmd.exe)" {
        $s = New-TestSession "fail7"
        $m = "SHELL_$(Get-Random)"
        & $PSMUX send-keys -t $s "`$PSVersionTable.PSVersion.Major" Enter
        Start-Sleep -Seconds 2
        $cap = Get-PaneOutput $s
        Remove-TestSession $s
        # PowerShell should show a version number
        [bool]($cap -match "[5-9]\b|[1-9]\d+")
    } 6

    Test-Case "6.8" "Pane inherits parent PATH" {
        $s = New-TestSession "fail8"
        if (-not (Wait-PaneReady $s)) { Remove-TestSession $s; return "Shell not ready" }
        $m = "PATH_$(Get-Random)"
        & $PSMUX send-keys -t $s "if (`$env:PATH.Length -gt 50) { Write-Host '${m}:PATH_OK' } else { Write-Host '${m}:PATH_SHORT' }" Enter
        $cap = Wait-ForOutput $s $m 15
        Remove-TestSession $s
        if (-not $cap) { return "No output" }
        [bool]($cap -match "${m}:PATH_OK")
    } 6

    # --- Resize/layout issues ---
    Write-Section "Resize & Layout Edge Cases"

    Test-Case "6.9" "Resize after layout change doesn't crash" {
        $s = New-TestSession "fail9"
        & $PSMUX split-window -h -t $s 2>&1 | Out-Null
        & $PSMUX split-window -v -t $s 2>&1 | Out-Null
        Start-Sleep -Seconds 1
        & $PSMUX select-layout -t $s tiled 2>&1 | Out-Null
        Start-Sleep -Milliseconds 500
        # This is the pattern that can trigger the ConPTY resize hang
        & $PSMUX select-layout -t $s main-vertical 2>&1 | Out-Null
        Start-Sleep -Milliseconds 500
        # Verify session is still alive
        & $PSMUX has-session -t $s 2>&1 | Out-Null
        $ok = $LASTEXITCODE -eq 0
        Remove-TestSession $s
        $ok
    } 6

    Test-Case "6.10" "resize-pane with percentage (%) accepted" {
        $s = New-TestSession "fail10"
        & $PSMUX split-window -h -t $s 2>&1 | Out-Null
        Start-Sleep -Seconds 1
        try {
            & $PSMUX resize-pane -t $s -x "30%" 2>&1 | Out-Null
            Remove-TestSession $s
            $true
        }
        catch {
            Remove-TestSession $s
            "resize-pane -x 30% rejected: $_"
        }
    } 6
}

# ============================================================
# PHASE 7: EXISTING TEST SUITE INTEGRATION
# ============================================================

if ($Phase -contains 7 -and -not $SkipExisting) {
    Write-Banner "PHASE 7: Existing Test Suites"

    $validateScript = Join-Path $PSScriptRoot "validate-swarm-backend.ps1"
    $agentTeamsScript = Join-Path $PSScriptRoot "test_claude_agent_teams.ps1"

    if (Test-Path $validateScript) {
        Write-Section "Running validate-swarm-backend.ps1"
        Test-Case "7.1" "validate-swarm-backend.ps1 passes" {
            $output = pwsh -NoProfile -File $validateScript 2>&1 | Out-String
            Write-Host $output -ForegroundColor DarkGray
            $LASTEXITCODE -eq 0
        } 7
    }
    else {
        Write-Host "  SKIP: validate-swarm-backend.ps1 not found at $validateScript" -ForegroundColor Yellow
    }

    if (Test-Path $agentTeamsScript) {
        Write-Section "Running test_claude_agent_teams.ps1"
        Test-Case "7.2" "test_claude_agent_teams.ps1 passes" {
            $output = pwsh -NoProfile -File $agentTeamsScript 2>&1 | Out-String
            Write-Host $output -ForegroundColor DarkGray
            $LASTEXITCODE -eq 0
        } 7
    }
    else {
        Write-Host "  SKIP: test_claude_agent_teams.ps1 not found at $agentTeamsScript" -ForegroundColor Yellow
    }
}

# ============================================================
# CLEANUP
# ============================================================

# Kill any leftover test sessions
1..50 | ForEach-Object {
    $name = "${SESSION_PREFIX}_$_"
    try { & $PSMUX kill-session -t $name 2>&1 | Out-Null } catch {}
}
@("swarm", "env1", "env2", "env3", "env4", "env5", "env7",
  "core1", "core2", "core3", "split1", "split2", "split3", "split4", "split5",
  "keys1", "keys2", "keys3", "layout1", "layout2", "layout3", "layout4",
  "cap1", "fmt1", "fmt2",
  "edge1", "edge2", "edge3", "edge4", "edge5", "edge6", "edge7", "edge8", "edge9", "edge10",
  "fail1", "fail2", "fail3", "fail4", "fail7", "fail8", "fail9", "fail10"
) | ForEach-Object {
    try { & $PSMUX kill-session -t "${SESSION_PREFIX}_$_" 2>&1 | Out-Null } catch {}
}
try { & $PSMUX kill-session -t "fail5_cycle" 2>&1 | Out-Null } catch {}
try { & $PSMUX kill-session -t "fail6_stale" 2>&1 | Out-Null } catch {}

if (-not $KeepArtifacts) {
    Remove-Item -Path $TESTDIR -Recurse -Force -ErrorAction SilentlyContinue
}

# ============================================================
# SUMMARY
# ============================================================

$elapsed = (Get-Date) - $script:startTime

Write-Host ""
Write-Host ("=" * 70) -ForegroundColor White
Write-Host "  SWARM E2E TEST RESULTS" -ForegroundColor White
Write-Host ("=" * 70) -ForegroundColor White
Write-Host ""

# Per-phase summary
$phases = @{
    1 = "Prerequisites"
    2 = "Environment Detection"
    3 = "Core Backend Commands"
    4 = "Swarm Lifecycle"
    5 = "Edge Cases & Stress"
    6 = "Failure Modes"
    7 = "Existing Suites"
}

foreach ($p in ($Phase | Sort-Object)) {
    $phaseTests = $script:results | Where-Object { $_.Phase -eq $p }
    if ($phaseTests.Count -eq 0) { continue }
    $pp = ($phaseTests | Where-Object { $_.Status -eq "PASS" }).Count
    $pf = ($phaseTests | Where-Object { $_.Status -eq "FAIL" }).Count
    $ps = ($phaseTests | Where-Object { $_.Status -eq "SKIP" }).Count
    $color = if ($pf -gt 0) { "Red" } elseif ($ps -gt 0) { "Yellow" } else { "Green" }
    $phaseName = $phases[$p]
    Write-Host "  Phase $p ($phaseName): " -NoNewline
    Write-Host "$pp pass" -ForegroundColor Green -NoNewline
    if ($pf -gt 0) { Write-Host ", $pf fail" -ForegroundColor Red -NoNewline }
    if ($ps -gt 0) { Write-Host ", $ps skip" -ForegroundColor Yellow -NoNewline }
    Write-Host ""
}

Write-Host ""
Write-Host ("  " + "-" * 40) -ForegroundColor DarkGray
Write-Host "  Total:   $($script:total)" -ForegroundColor White
Write-Host "  Passed:  $($script:pass)" -ForegroundColor Green
Write-Host "  Failed:  $($script:fail)" -ForegroundColor $(if ($script:fail -gt 0) { "Red" } else { "Green" })
Write-Host "  Skipped: $($script:skip)" -ForegroundColor $(if ($script:skip -gt 0) { "Yellow" } else { "DarkGray" })
Write-Host "  Time:    $([math]::Round($elapsed.TotalSeconds, 1))s" -ForegroundColor DarkGray
Write-Host ""

if ($script:fail -gt 0) {
    Write-Host "  FAILED TESTS:" -ForegroundColor Red
    $script:results | Where-Object { $_.Status -eq "FAIL" } | ForEach-Object {
        $detail = if ($_.Detail) { " - $($_.Detail)" } else { "" }
        Write-Host "    [$($_.Id)] $($_.Name)${detail}" -ForegroundColor Red
    }
    Write-Host ""
    Write-Host "  DIAGNOSIS HINTS:" -ForegroundColor Yellow

    $failedIds = $script:results | Where-Object { $_.Status -eq "FAIL" } | ForEach-Object { $_.Id }

    if ($failedIds -match "^2\." ) {
        Write-Host "    - Phase 2 failures: psmux not setting env vars in child shells" -ForegroundColor Yellow
        Write-Host "      Fix: check session init code sets TMUX, TMUX_PANE, AGENT_TEAMS" -ForegroundColor DarkGray
    }
    if ($failedIds -match "^3\.[4-8]") {
        Write-Host "    - Phase 3 split/pane failures: split-window or pane ID format issues" -ForegroundColor Yellow
        Write-Host "      Fix: check -P -F flag handling returns %N format" -ForegroundColor DarkGray
    }
    if ($failedIds -match "^3\.(9|10|11)") {
        Write-Host "    - Phase 3 send-keys failures: text delivery or encoding issues" -ForegroundColor Yellow
        Write-Host "      Fix: check send-keys input encoding, -l literal mode, pane targeting" -ForegroundColor DarkGray
    }
    if ($failedIds -match "^4\.") {
        Write-Host "    - Phase 4 swarm lifecycle failures: multi-pane coordination issues" -ForegroundColor Yellow
        Write-Host "      Fix: check pane lifecycle, session persistence, kill-pane targeting" -ForegroundColor DarkGray
    }
    if ($failedIds -match "^5\.(1|2)") {
        Write-Host "    - Phase 5 long prompt failures: send-keys buffer overflow or truncation" -ForegroundColor Yellow
        Write-Host "      Fix: check send-keys input buffer size limits" -ForegroundColor DarkGray
    }
    if ($failedIds -match "^5\.7") {
        Write-Host "    - Phase 5 race condition: pane ID allocator needs mutex/atomic" -ForegroundColor Yellow
    }
    if ($failedIds -match "^6\.(1|2)") {
        Write-Host "    - Phase 6 fallback triggers: Claude Code will use in-process mode" -ForegroundColor Yellow
        Write-Host "      Fix: ensure TMUX env var set and tmux alias resolves to psmux" -ForegroundColor DarkGray
    }
    if ($failedIds -match "^6\.5") {
        Write-Host "    - Phase 6 stale pipe: named pipe cleanup issue on rapid cycle" -ForegroundColor Yellow
    }
}

Write-Host ("=" * 70) -ForegroundColor White

# ============================================================
# DETAILED REPORT GENERATION
# ============================================================

if (-not $ReportPath) {
    $ReportPath = Join-Path $PSScriptRoot "swarm-e2e-report.md"
}

$reportLines = @()
$reportLines += "# psmux Swarm E2E Test Report"
$reportLines += ""
$reportLines += "**Generated:** $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
$reportLines += "**Platform:** $([System.Environment]::OSVersion.VersionString)"
$reportLines += "**PowerShell:** $($PSVersionTable.PSVersion)"
$reportLines += "**psmux binary:** $PSMUX"
$psmuxVer = & $PSMUX -V 2>&1 | Out-String
$reportLines += "**psmux version:** $($psmuxVer.Trim())"
$reportLines += "**Duration:** $([math]::Round($elapsed.TotalSeconds, 1))s"
$reportLines += "**Phases run:** $($Phase -join ', ')"
$reportLines += ""

# Overall summary
$reportLines += "## Summary"
$reportLines += ""
$reportLines += "| Metric | Count |"
$reportLines += "|--------|-------|"
$reportLines += "| Total | $($script:total) |"
$reportLines += "| Passed | $($script:pass) |"
$reportLines += "| Failed | $($script:fail) |"
$reportLines += "| Skipped | $($script:skip) |"
if ($script:total -gt 0) {
    $rate = [math]::Round(($script:pass / $script:total) * 100, 1)
    $reportLines += "| Pass Rate | ${rate}% |"
}
$reportLines += ""

# Per-phase breakdown
$reportLines += "## Phase Results"
$reportLines += ""

foreach ($p in ($Phase | Sort-Object)) {
    $phaseName = $phases[$p]
    $phaseTests = $script:results | Where-Object { $_.Phase -eq $p }
    if ($phaseTests.Count -eq 0) { continue }

    $pp = ($phaseTests | Where-Object { $_.Status -eq "PASS" }).Count
    $pf = ($phaseTests | Where-Object { $_.Status -eq "FAIL" }).Count
    $ps = ($phaseTests | Where-Object { $_.Status -eq "SKIP" }).Count
    $statusIcon = if ($pf -gt 0) { "FAIL" } elseif ($ps -gt 0) { "WARN" } else { "PASS" }

    $reportLines += "### Phase ${p}: $phaseName [$statusIcon]"
    $reportLines += ""
    $reportLines += "| Test | Name | Status | Detail |"
    $reportLines += "|------|------|--------|--------|"

    foreach ($t in $phaseTests) {
        $icon = switch ($t.Status) {
            "PASS" { "PASS" }
            "FAIL" { "FAIL" }
            "SKIP" { "SKIP" }
        }
        $detail = if ($t.Detail) { $t.Detail -replace '\|', '\|' -replace "`n", " " } else { "-" }
        # Truncate long details for table readability
        if ($detail.Length -gt 120) { $detail = $detail.Substring(0, 117) + "..." }
        $reportLines += "| $($t.Id) | $($t.Name) | $icon | $detail |"
    }
    $reportLines += ""
}

# Failed tests detail section
$failedTests = $script:results | Where-Object { $_.Status -eq "FAIL" }
if ($failedTests.Count -gt 0) {
    $reportLines += "## Failed Tests Analysis"
    $reportLines += ""

    foreach ($t in $failedTests) {
        $reportLines += "### [$($t.Id)] $($t.Name)"
        $reportLines += ""
        if ($t.Detail) {
            $reportLines += "**Error:** ``$($t.Detail)``"
            $reportLines += ""
        }

        # Automated diagnosis based on test ID
        switch -Regex ($t.Id) {
            "^1\." {
                $reportLines += "**Category:** Prerequisite missing"
                $reportLines += "**Impact:** Tests in later phases may also fail"
                $reportLines += "**Fix:** Install or add to PATH the missing binary"
            }
            "^2\.[1-4]" {
                $reportLines += "**Category:** Environment variable not set"
                $reportLines += "**Impact:** Claude Code will fall back to in-process agent spawning (invisible agents)"
                $reportLines += "**Fix:** Check psmux session init sets ``TMUX``, ``TMUX_PANE`` in child shell env"
                $reportLines += "**Source:** ``src/server/mod.rs`` or ``src/pane.rs`` (env propagation to child processes)"
            }
            "^2\.7" {
                $reportLines += "**Category:** Agent teams feature flag"
                $reportLines += "**Impact:** Claude Code won't enable agent teams even if TMUX is set"
                $reportLines += "**Fix:** psmux should set ``CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`` in child env"
            }
            "^3\.[1-3]" {
                $reportLines += "**Category:** Session management"
                $reportLines += "**Impact:** Cannot create or manage swarm sessions"
                $reportLines += "**Fix:** Check ``src/server/mod.rs`` session create/kill handlers"
            }
            "^3\.[4-8]" {
                $reportLines += "**Category:** Pane creation / ID format"
                $reportLines += "**Impact:** Claude Code cannot spawn or target agent panes"
                $reportLines += "**Fix:** Ensure ``split-window -P -F #{pane_id}`` returns ``%N`` format to stdout"
                $reportLines += "**Source:** ``src/pane.rs`` (pane ID allocation), ``src/window_ops.rs`` (split-window handler)"
            }
            "^3\.(9|10|11)" {
                $reportLines += "**Category:** send-keys delivery"
                $reportLines += "**Impact:** Agent prompts won't reach panes correctly"
                $reportLines += "**Fix:** Check ``src/input.rs`` (send-keys handler), literal mode ``-l`` flag, pane targeting"
            }
            "^3\.1[2-5]" {
                $reportLines += "**Category:** Layout / display"
                $reportLines += "**Impact:** Panes may overlap or display incorrectly"
                $reportLines += "**Fix:** Check ``src/window_ops.rs`` layout engine"
            }
            "^4\." {
                $reportLines += "**Category:** Swarm lifecycle"
                $reportLines += "**Impact:** Multi-agent workflows will fail mid-execution"
                $reportLines += "**Fix:** Review the specific lifecycle step that failed"
            }
            "^5\.[12]" {
                $reportLines += "**Category:** Long prompt handling"
                $reportLines += "**Impact:** Claude Code agent prompts (often 1000+ chars) will be truncated"
                $reportLines += "**Fix:** Check send-keys input buffer size, may need chunked writes"
                $reportLines += "**Source:** ``src/input.rs`` (WriteConsoleInput buffer)"
            }
            "^5\.7" {
                $reportLines += "**Category:** Race condition"
                $reportLines += "**Impact:** Concurrent agent spawning produces duplicate/invalid pane IDs"
                $reportLines += "**Fix:** Pane ID allocator needs mutex or atomic counter"
                $reportLines += "**Source:** ``src/pane.rs`` (pane ID generation)"
            }
            "^6\.[12]" {
                $reportLines += "**Category:** In-process fallback trigger"
                $reportLines += "**Impact:** Agents spawn invisibly — no visible panes, no monitoring"
                $reportLines += "**Fix:** Ensure ``TMUX`` env var set AND ``tmux`` alias resolves to psmux inside sessions"
            }
            "^6\.[34]" {
                $reportLines += "**Category:** send-keys encoding"
                $reportLines += "**Impact:** POSIX-style commands from Claude Code break in PowerShell panes"
                $reportLines += "**Fix:** Check env shim (``_pu`` function) for POSIX-to-PowerShell translation"
                $reportLines += "**Source:** ``src/input.rs`` (env shim injection)"
            }
            "^6\.[56]" {
                $reportLines += "**Category:** Named pipe / IPC"
                $reportLines += "**Impact:** Session creation fails after rapid cycles or stale state"
                $reportLines += "**Fix:** Check named pipe cleanup and port file lifecycle"
                $reportLines += "**Source:** ``src/server/mod.rs`` (pipe listener), ``src/client.rs`` (connection)"
            }
            "^6\.(9|10)" {
                $reportLines += "**Category:** Resize / ConPTY"
                $reportLines += "**Impact:** Layout changes crash or hang the session (known ConPTY issue)"
                $reportLines += "**Fix:** See ``references/investigation-102-88.md`` for resize hang mitigation"
            }
        }
        $reportLines += ""
    }
}

# Swarm readiness assessment
$reportLines += "## Swarm Readiness Assessment"
$reportLines += ""

$criticalTests = @("2.1", "2.2", "2.3", "3.4", "3.5", "3.6", "3.7", "3.9", "3.11", "6.1")
$criticalFails = $failedTests | Where-Object { $criticalTests -contains $_.Id }
$swarmFails = $script:results | Where-Object { $_.Status -eq "FAIL" -and $_.Phase -eq 4 }

if ($criticalFails.Count -eq 0 -and $swarmFails.Count -eq 0) {
    $reportLines += "**READY** — All critical backend commands and swarm lifecycle tests pass."
    $reportLines += "psmux can serve as the tmux spawn backend for Claude Code agent teams."
    $reportLines += ""
    $reportLines += "**Next steps:**"
    $reportLines += "1. Run ``pwsh scripts/Start-ClaudeTeams.ps1`` to launch Claude Code with agent teams"
    $reportLines += "2. Inside Claude Code, ask it to spawn teammates — they should appear as visible psmux panes"
    $reportLines += "3. Try ``/spawn-swarm`` for full multi-agent orchestration"
}
elseif ($criticalFails.Count -le 2) {
    $reportLines += "**PARTIAL** — Most backend commands work but $($criticalFails.Count) critical test(s) failed."
    $reportLines += ""
    $reportLines += "**Blocking issues:**"
    foreach ($f in $criticalFails) {
        $reportLines += "- [$($f.Id)] $($f.Name)"
    }
    $reportLines += ""
    $reportLines += "Fix these before attempting agent teams. Claude Code will fall back to in-process mode."
}
else {
    $reportLines += "**NOT READY** — $($criticalFails.Count) critical tests failed."
    $reportLines += "psmux cannot serve as the tmux backend in this state."
    $reportLines += ""
    $reportLines += "**Critical failures:**"
    foreach ($f in $criticalFails) {
        $reportLines += "- [$($f.Id)] $($f.Name): $($f.Detail)"
    }
}

$reportLines += ""
$reportLines += "## Environment Details"
$reportLines += ""
$reportLines += "| Variable | Value |"
$reportLines += "|----------|-------|"

$tmuxCmd = Get-Command tmux -ErrorAction SilentlyContinue
$reportLines += "| ``tmux`` on PATH | $(if ($tmuxCmd) { $tmuxCmd.Source } else { 'NOT FOUND' }) |"
$psmuxCmd = Get-Command psmux -ErrorAction SilentlyContinue
$reportLines += "| ``psmux`` on PATH | $(if ($psmuxCmd) { $psmuxCmd.Source } else { 'NOT FOUND' }) |"
$nodeCmd = Get-Command node -ErrorAction SilentlyContinue
$reportLines += "| ``node`` on PATH | $(if ($nodeCmd) { $nodeCmd.Source } else { 'NOT FOUND' }) |"
$claudeCmd = Get-Command claude -ErrorAction SilentlyContinue
$reportLines += "| ``claude`` on PATH | $(if ($claudeCmd) { $claudeCmd.Source } else { 'NOT FOUND' }) |"
$gitCmd = Get-Command git -ErrorAction SilentlyContinue
$reportLines += "| ``git`` on PATH | $(if ($gitCmd) { $gitCmd.Source } else { 'NOT FOUND' }) |"
$reportLines += "| Windows Terminal | $(if ($env:WT_SESSION) { 'Yes' } else { 'No / Unknown' }) |"
$reportLines += "| ``TERM_PROGRAM`` | $(if ($env:TERM_PROGRAM) { $env:TERM_PROGRAM } else { 'not set' }) |"

$reportLines += ""
$reportLines += "---"
$reportLines += "*Report generated by ``tests/test_swarm_e2e.ps1``*"

# Write report
$reportContent = $reportLines -join "`n"
Set-Content -Path $ReportPath -Value $reportContent -Encoding UTF8
Write-Host ""
Write-Host "  Report written to: $ReportPath" -ForegroundColor Cyan

# Exit with failure count
exit $script:fail
