# tests/test_interprocess_pipe_spike.ps1
#
# SPIKE contract test for src/backend/pipe_interprocess.rs (the parallel
# `interprocess`-crate-based named-pipe backend, behind --features
# interprocess-pipe). Compares behavior against the existing hand-rolled
# backend at src/backend/pipe.rs (commit aaae38f drop-guard semantics).
#
# Usage: pwsh tests/test_interprocess_pipe_spike.ps1
# Pre-req: Run from repo root (or absolute paths). Windows-only.

$ErrorActionPreference = 'Continue'

# ── Repo root resolution ────────────────────────────────────────────────────
$RepoRoot = (Resolve-Path "$PSScriptRoot/..").Path
$SpikeBin = Join-Path $RepoRoot 'target/spike-interprocess/debug/psmux.exe'
$DefaultBin = Join-Path $RepoRoot 'target/debug/psmux.exe'

$results = @()
$passCount = 0
$failCount = 0
$sessionsToCleanup = @()

function Test-Case {
    param([string]$Name, [scriptblock]$Test)
    Write-Host "`n--- $Name ---" -ForegroundColor Cyan
    try {
        $ok = & $Test
        if ($ok -eq $true) {
            $script:passCount++
            $script:results += [pscustomobject]@{ Name = $Name; Status = 'PASS' }
            Write-Host "  PASS" -ForegroundColor Green
        } else {
            $script:failCount++
            $script:results += [pscustomobject]@{ Name = $Name; Status = 'FAIL'; Detail = 'returned non-true' }
            Write-Host "  FAIL: returned non-true" -ForegroundColor Red
        }
    } catch {
        $script:failCount++
        $script:results += [pscustomobject]@{ Name = $Name; Status = 'FAIL'; Detail = $_.Exception.Message }
        Write-Host "  FAIL: $($_.Exception.Message)" -ForegroundColor Red
    }
}

function Cleanup-Sessions {
    foreach ($s in $script:sessionsToCleanup) {
        & $script:SpikeBin kill-session -t $s 2>$null | Out-Null
        & $script:DefaultBin kill-session -t $s 2>$null | Out-Null
    }
}

# ── JSON-RPC client over named pipe ─────────────────────────────────────────
# Sends one newline-delimited JSON-RPC request and reads one JSON line back.
function Invoke-PipeRpc {
    param(
        [Parameter(Mandatory=$true)][string]$PipeName,    # bare name, no \\.\pipe\ prefix
        [Parameter(Mandatory=$true)][string]$RequestJson,
        [int]$ConnectTimeoutMs = 3000,
        [int]$ReadTimeoutMs = 3000
    )
    $client = New-Object System.IO.Pipes.NamedPipeClientStream(
        '.', $PipeName,
        [System.IO.Pipes.PipeDirection]::InOut,
        [System.IO.Pipes.PipeOptions]::None)
    try {
        $client.Connect($ConnectTimeoutMs)
        $client.ReadMode = [System.IO.Pipes.PipeTransmissionMode]::Byte
        $writer = New-Object System.IO.StreamWriter($client)
        $writer.NewLine = "`n"
        $writer.AutoFlush = $true
        $writer.WriteLine($RequestJson)
        $reader = New-Object System.IO.StreamReader($client)
        # Read one line with timeout via async read task
        $readTask = $reader.ReadLineAsync()
        if ($readTask.Wait($ReadTimeoutMs)) {
            return $readTask.Result
        } else {
            throw "RPC read timed out after $ReadTimeoutMs ms"
        }
    } finally {
        $client.Dispose()
    }
}

# ── Wait helpers ────────────────────────────────────────────────────────────
function Wait-Pipe {
    param([string]$PipeName, [int]$TimeoutMs = 5000)
    $deadline = (Get-Date).AddMilliseconds($TimeoutMs)
    while ((Get-Date) -lt $deadline) {
        if ([System.IO.File]::Exists("\\.\pipe\$PipeName")) { return $true }
        Start-Sleep -Milliseconds 100
    }
    return $false
}

# ════════════════════════════════════════════════════════════════════════════
# CONTRACT 1: BUILD
# `cargo build --features interprocess-pipe` succeeds and produces a binary
# whose backend module exposes pipe_interprocess.
# ════════════════════════════════════════════════════════════════════════════
Test-Case "Contract 1: BUILD --features interprocess-pipe" {
    Push-Location $RepoRoot
    try {
        $env:CARGO_TARGET_DIR = (Join-Path $RepoRoot 'target/spike-interprocess')
        $out = cargo build --features interprocess-pipe --bin psmux 2>&1
        $code = $LASTEXITCODE
        Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
        if ($code -ne 0) {
            Write-Host ($out | Out-String)
            return $false
        }
        return (Test-Path $SpikeBin)
    } finally {
        Pop-Location
    }
}

# ════════════════════════════════════════════════════════════════════════════
# CONTRACT 2: SMOKE — JSON-RPC `list` round-trip on the spike backend.
# Ensures the interprocess-crate listener accepts a connection, parses
# newline-delimited JSON-RPC, and returns a well-formed response.
# ════════════════════════════════════════════════════════════════════════════
Test-Case "Contract 2: SMOKE — list RPC round-trip on spike backend" {
    if (-not (Test-Path $SpikeBin)) {
        Write-Host "  spike binary missing — Contract 1 must pass first" -ForegroundColor Yellow
        return $false
    }
    $session = "spike-smoke-$([guid]::NewGuid().ToString('N').Substring(0,8))"
    $pipeName = "psmux-claude-backend-$session"
    $script:sessionsToCleanup += $session

    # Boot a detached session — pipe listener starts at session start.
    & $SpikeBin new-session -d -s $session 2>&1 | Out-Null
    if (-not (Wait-Pipe -PipeName $pipeName -TimeoutMs 5000)) {
        Write-Host "  pipe never appeared at \\.\pipe\$pipeName" -ForegroundColor Red
        return $false
    }

    $req = '{"jsonrpc":"2.0","id":1,"method":"list","params":{}}'
    $resp = Invoke-PipeRpc -PipeName $pipeName -RequestJson $req
    Write-Host "  resp: $resp"
    if (-not $resp) { return $false }
    $obj = $resp | ConvertFrom-Json -ErrorAction Stop
    # Schema validation at the boundary (Pattern 1):
    if ($obj.id -ne 1) {
        Write-Host "  id mismatch: got $($obj.id)" -ForegroundColor Red
        return $false
    }
    if ($null -eq $obj.result -and $null -eq $obj.error) {
        Write-Host "  response missing both result and error" -ForegroundColor Red
        return $false
    }
    return $true
}

# ════════════════════════════════════════════════════════════════════════════
# CONTRACT 3: DROP-GUARD — repeated connect/disconnect must not exhaust
# handles or leak writer threads. This is the property aaae38f restored on
# the hand-rolled backend; must hold on the interprocess port too.
# Pragmatic proxy: open + close 10 connections, then assert connection 11
# still works within the same timeout budget.
# ════════════════════════════════════════════════════════════════════════════
Test-Case "Contract 3: DROP-GUARD — 10 disconnect cycles, then connection 11 still serves" {
    if (-not (Test-Path $SpikeBin)) { return $false }
    $session = "spike-drop-$([guid]::NewGuid().ToString('N').Substring(0,8))"
    $pipeName = "psmux-claude-backend-$session"
    $script:sessionsToCleanup += $session

    & $SpikeBin new-session -d -s $session 2>&1 | Out-Null
    if (-not (Wait-Pipe -PipeName $pipeName -TimeoutMs 5000)) { return $false }

    $req = '{"jsonrpc":"2.0","id":42,"method":"list","params":{}}'
    for ($i = 1; $i -le 10; $i++) {
        $r = Invoke-PipeRpc -PipeName $pipeName -RequestJson $req -ReadTimeoutMs 2000
        if (-not $r) {
            Write-Host "  cycle $i returned empty" -ForegroundColor Red
            return $false
        }
    }
    # Give writer threads a beat to drop after disconnect (drop-guard contract: <1s).
    Start-Sleep -Milliseconds 500

    # Connection 11 — the leak test. If handles or threads leaked, this hangs
    # or fails. ReadTimeoutMs caps the failure mode.
    $final = Invoke-PipeRpc -PipeName $pipeName -RequestJson $req -ReadTimeoutMs 3000
    if (-not $final) {
        Write-Host "  connection 11 failed — likely handle exhaustion" -ForegroundColor Red
        return $false
    }
    $obj = $final | ConvertFrom-Json -ErrorAction Stop
    return ($obj.id -eq 42)
}

# ════════════════════════════════════════════════════════════════════════════
# CONTRACT 4: REGRESSION — default-feature build (interprocess OFF) must
# still pass the existing swarm-backend validation suite.
# ════════════════════════════════════════════════════════════════════════════
Test-Case "Contract 4: REGRESSION — default build still passes validate-swarm-backend.ps1" {
    Push-Location $RepoRoot
    try {
        # Build default-feature binary
        $out = cargo build --bin psmux 2>&1
        if ($LASTEXITCODE -ne 0) {
            Write-Host ($out | Out-String)
            return $false
        }
        if (-not (Test-Path $DefaultBin)) {
            Write-Host "  default binary missing at $DefaultBin" -ForegroundColor Red
            return $false
        }
        # Put default binary first on PATH for the validator.
        $oldPath = $env:PATH
        $env:PATH = "$(Split-Path $DefaultBin);$env:PATH"
        try {
            $script = Join-Path $RepoRoot 'tests/validate-swarm-backend.ps1'
            & pwsh -NoProfile -File $script *>&1 | Tee-Object -Variable validatorOut | Out-Host
            return ($LASTEXITCODE -eq 0)
        } finally {
            $env:PATH = $oldPath
        }
    } finally {
        Pop-Location
    }
}

# ── Cleanup + report ────────────────────────────────────────────────────────
Cleanup-Sessions

Write-Host "`n══════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host " SPIKE TEST RESULTS" -ForegroundColor Cyan
Write-Host "══════════════════════════════════════════════════" -ForegroundColor Cyan
$results | Format-Table -AutoSize | Out-String | Write-Host
Write-Host " Pass: $passCount   Fail: $failCount" -ForegroundColor $(if ($failCount -eq 0) { 'Green' } else { 'Red' })
exit $(if ($failCount -eq 0) { 0 } else { 1 })
