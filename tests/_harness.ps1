# tests/_harness.ps1
#
# Shared harness for rails-bench ps1 tests.  Dot-source from each test file:
#
#     . $PSScriptRoot/_harness.ps1
#     Test-Case "description" { ... returns $true/$false }
#     $S = New-IsolatedSession "my-test"
#     Remove-PsmuxSession $S
#     Write-Summary
#
# Keeps per-file tests uniform without pulling in a heavyweight framework.
# Silent on success, loud on failure.  Exit code = failure count.

$ErrorActionPreference = 'Continue'

# Lift the agent env vars so nested sessions work under test harness.
# PSMUX_TARGET_SESSION is cleared too — otherwise a stale value from the
# parent bench runner can mis-route commands to a prior test's server.
$env:PSMUX_SESSION = $null
$env:PSMUX_TARGET_SESSION = $null
$env:PSMUX_TARGET_FULL = $null
$env:TMUX = $null
$env:PSMUX_ACTIVE = $null

# Per-run counters (script-scoped — one counter set per dot-sourcing file)
if (-not (Get-Variable -Scope Script -Name RailsBenchCounters -ErrorAction SilentlyContinue)) {
    $script:RailsBenchCounters = @{ Total = 0; Pass = 0; Fail = 0; Failures = @() }
}

function Test-Case {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][scriptblock]$Body
    )
    $script:RailsBenchCounters.Total++
    try {
        $result = & $Body
        # Accept explicit $true, non-empty/non-zero values that coerce to true.
        # Reject $null, $false, 0, empty string.
        if ($result -eq $true) {
            Write-Host ("  + {0}" -f $Name) -ForegroundColor Green
            $script:RailsBenchCounters.Pass++
        } else {
            Write-Host ("  X {0}" -f $Name) -ForegroundColor Red
            Write-Host ("    body returned: {0}" -f ($result | Out-String).Trim()) -ForegroundColor DarkYellow
            $script:RailsBenchCounters.Fail++
            $script:RailsBenchCounters.Failures += $Name
        }
    } catch {
        Write-Host ("  X {0} (threw)" -f $Name) -ForegroundColor Red
        Write-Host ("    {0}" -f $_.Exception.Message) -ForegroundColor DarkYellow
        $script:RailsBenchCounters.Fail++
        $script:RailsBenchCounters.Failures += "$Name (threw)"
    }
}

function New-IsolatedSession {
    param(
        [Parameter(Mandatory)][string]$Prefix,
        [string]$InitShell = $null
    )
    # Unique session name so parallel test runs don't collide
    $suffix = [guid]::NewGuid().Guid.Substring(0, 8)
    $name = "$Prefix-$suffix"
    $args = @('new-session', '-d', '-s', $name)
    if ($InitShell) { $args += @('--shell', $InitShell) }
    $null = psmux @args 2>&1
    Start-Sleep -Milliseconds 400  # let warm shell settle
    return $name
}

function Remove-PsmuxSession {
    param([Parameter(Mandatory)][string]$Session)
    $null = psmux kill-session -t $Session 2>&1
}

function Wait-ForOutput {
    param(
        [Parameter(Mandatory)][string]$Target,
        [Parameter(Mandatory)][string]$Pattern,
        [int]$TimeoutMs = 5000
    )
    # Prefer server-side wait-for; fall back to polling capture-pane if wait-for errors.
    # WaitOutcome is serialised with `#[serde(tag = "kind", rename_all = "snake_case")]`,
    # so the discriminant is `kind` ∈ success|exit_success|timeout|error — NOT `outcome`.
    # Checking a non-existent `.outcome` returned $null and made this helper report
    # failure on exactly the fast path where wait-for had succeeded.
    $result = psmux wait-for -t $Target --output $Pattern --timeout $TimeoutMs --json 2>&1
    if ($LASTEXITCODE -eq 0) {
        try {
            $kind = ($result | ConvertFrom-Json).kind
            return ($kind -eq 'success' -or $kind -eq 'exit_success')
        } catch { }
    }
    # Fallback: 10 poll ticks
    $deadline = (Get-Date).AddMilliseconds($TimeoutMs)
    while ((Get-Date) -lt $deadline) {
        $text = psmux capture-pane -t $Target -p 2>$null
        if ($text -match [regex]::Escape($Pattern)) { return $true }
        Start-Sleep -Milliseconds 200
    }
    return $false
}

function Write-Summary {
    $c = $script:RailsBenchCounters
    Write-Host ""
    if ($c.Fail -eq 0) {
        Write-Host ("=== {0}/{1} passed ===" -f $c.Pass, $c.Total) -ForegroundColor Green
    } else {
        Write-Host ("=== {0}/{1} passed — {2} FAILURES ===" -f $c.Pass, $c.Total, $c.Fail) -ForegroundColor Red
        $c.Failures | ForEach-Object { Write-Host ("  - {0}" -f $_) -ForegroundColor Red }
    }
    exit $c.Fail
}
