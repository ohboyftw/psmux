# Rails bench: PSMUX_DATA_DIR moves the whole data directory.
#
# Every consumer used to resolve the root itself, mixing USERPROFILE, HOME and
# hardcoded separators across 19 files, so there was no single place to point
# somewhere else. They now route through paths.rs, and an absolute
# PSMUX_DATA_DIR takes precedence — an embedding application can keep server
# state, logs and session files inside its own install root.
#
# The test asserts the redirect AND the isolation: a session created under a
# custom data dir must be invisible to a client using the default one, because
# that is what "moved" has to mean for the port files the client scans.
. $PSScriptRoot/_harness.ps1

Write-Host "== PSMUX_DATA_DIR ==" -ForegroundColor Cyan

$dataDir = Join-Path $env:TEMP "psmux-datadir-$([guid]::NewGuid().Guid.Substring(0,8))"
New-Item -ItemType Directory -Path $dataDir -Force | Out-Null
$S = "datadir-$([guid]::NewGuid().Guid.Substring(0,6))"
$defaultDir = Join-Path $env:USERPROFILE ".psmux"

$env:PSMUX_DATA_DIR = $dataDir
$null = psmux new-session -d -s $S 2>&1
Start-Sleep -Milliseconds 1200

Test-Case "the session's port file lands in the custom data dir" {
    $port = Join-Path $dataDir "$S.port"
    if (-not (Test-Path $port)) {
        Write-Host "    contents: $((Get-ChildItem $dataDir -ErrorAction SilentlyContinue).Name -join ', ')" -ForegroundColor DarkYellow
    }
    Test-Path $port
}

Test-Case "nothing was written to the default data dir" {
    -not (Test-Path (Join-Path $defaultDir "$S.port"))
}

Test-Case "a client sharing the data dir sees the session" {
    $out = psmux list-sessions 2>&1
    ($out -join "`n") -match [regex]::Escape($S)
}

Test-Case "a client on the default data dir does not see it" {
    $keep = $env:PSMUX_DATA_DIR
    $env:PSMUX_DATA_DIR = $null
    $out = psmux list-sessions 2>&1
    $env:PSMUX_DATA_DIR = $keep
    -not (($out -join "`n") -match [regex]::Escape($S))
}

Test-Case "kill-session removes the port file from the custom data dir" {
    $null = psmux kill-session -t $S 2>&1
    Start-Sleep -Milliseconds 800
    -not (Test-Path (Join-Path $dataDir "$S.port"))
}

$env:PSMUX_DATA_DIR = $null
Remove-Item -LiteralPath $dataDir -Recurse -Force -ErrorAction SilentlyContinue
Write-Summary
