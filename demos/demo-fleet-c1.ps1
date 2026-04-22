# demo-fleet-c1.ps1 — FleetView C1 end-to-end demo.
#
# Runs a real `psmux orchestrate` plan (3 workers from examples/orchestrate/
# hello-world.json), then queries the produced state.json through every
# FleetView surface: `psmux fleet ls`, `psmux fleet status`, and the
# `psmux-fleet-mcp` stdio JSON-RPC server.
#
# Usage:
#   pwsh demos/demo-fleet-c1.ps1              # run against target/debug binaries
#   pwsh demos/demo-fleet-c1.ps1 -Release     # use target/release instead
#
# Prerequisites: `cargo build --workspace` (or `--release` if -Release).

param(
    [switch]$Release,
    [switch]$KeepTemp
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$profileDir = if ($Release) { "release" } else { "debug" }
$psmux = Join-Path $repoRoot "target\$profileDir\psmux.exe"
$mcp   = Join-Path $repoRoot "target\$profileDir\psmux-fleet-mcp.exe"
$src   = Join-Path $repoRoot "examples\orchestrate\hello-world.json"

function Assert-Binary($path) {
    if (-not (Test-Path $path)) {
        throw "Binary not found: $path`nRun ``cargo build$(if ($Release) { ' --release' })`` first."
    }
}

function Header($text) {
    Write-Host ""
    Write-Host "=== $text ===" -ForegroundColor Cyan
}

Assert-Binary $psmux
Assert-Binary $mcp
if (-not (Test-Path $src)) { throw "Fixture plan not found: $src" }

# Unique session to avoid collision with any stale orc-hello session.
$session = "fleetview-demo-$PID"
$workDir = Join-Path $env:TEMP "fleet-demo-$PID"
New-Item -ItemType Directory -Force -Path $workDir | Out-Null
$plan = Join-Path $workDir "plan.json"
Copy-Item $src $plan

Write-Host "FleetView C1 demo" -ForegroundColor Green
Write-Host "  psmux : $psmux"
Write-Host "  mcp   : $mcp"
Write-Host "  plan  : $plan"
Write-Host "  session: $session"

try {
    # 1. Create a detached psmux session for orchestrate to spawn panes into.
    & $psmux new-session -d -s $session 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "failed to create psmux session $session (exit $LASTEXITCODE)"
    }

    # 2. Run orchestrate to completion. --json makes it block until done and
    # write the final state.json to stdout. We only care about the exit
    # code; the on-disk state.json is what the FleetView surfaces read.
    Header "psmux orchestrate"
    & $psmux orchestrate $plan --session $session --json --timeout 30000 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "orchestrate failed with exit $LASTEXITCODE"
    }
    Write-Host "  (3 workers ran to completion)"

    $stateJson = Join-Path $workDir ".orchestration\$session\state.json"
    if (-not (Test-Path $stateJson)) {
        throw "state.json not found at $stateJson"
    }

    # 2. CLI — human-readable table.
    Header "psmux fleet ls"
    & $psmux fleet ls --state-path $stateJson

    # 3. CLI — machine-readable.
    Header "psmux fleet ls --json"
    $cliJson = & $psmux fleet ls --state-path $stateJson --json | Out-String
    $cliJson

    $cliParsed = $cliJson | ConvertFrom-Json
    if ($cliParsed.total -ne 3) { throw "CLI: expected total=3, got $($cliParsed.total)" }

    # 4. CLI — detail view for one worker.
    Header "psmux fleet status greeter"
    & $psmux fleet status greeter --state-path $stateJson

    Header "psmux fleet status greeter --json"
    & $psmux fleet status greeter --state-path $stateJson --json

    # 5. CLI — unknown id hits WORKSPACE_NOT_FOUND.
    Header "psmux fleet status nonexistent (expect error, exit 1)"
    & $psmux fleet status nonexistent --state-path $stateJson 2>&1
    if ($LASTEXITCODE -ne 1) {
        throw "expected exit 1 for unknown id, got $LASTEXITCODE"
    }
    Write-Host "  -> got exit 1 as expected"

    # 6. MCP — fleet.list.
    Header "psmux-fleet-mcp  fleet.list"
    $req = '{"jsonrpc":"2.0","id":1,"method":"fleet.list","params":{}}'
    $mcpResp = ($req | & $mcp --state-path $stateJson 2>$null | Select-Object -First 1)
    $mcpResp
    $mcpParsed = $mcpResp | ConvertFrom-Json
    if ($mcpParsed.result.total -ne 3) { throw "MCP: expected result.total=3, got $($mcpParsed.result.total)" }

    # 7. MCP — fleet.status.
    Header "psmux-fleet-mcp  fleet.status greeter"
    $req2 = '{"jsonrpc":"2.0","id":2,"method":"fleet.status","params":{"id":"greeter"}}'
    ($req2 | & $mcp --state-path $stateJson 2>$null | Select-Object -First 1)

    # 8. MCP — unknown method surfaces an error envelope.
    Header "psmux-fleet-mcp  fleet.nope (expect METHOD_NOT_FOUND)"
    $req3 = '{"jsonrpc":"2.0","id":3,"method":"fleet.nope","params":{}}'
    $nopeResp = ($req3 | & $mcp --state-path $stateJson 2>$null | Select-Object -First 1)
    $nopeResp
    $nopeParsed = $nopeResp | ConvertFrom-Json
    if ($nopeParsed.error.code -ne "METHOD_NOT_FOUND") {
        throw "expected METHOD_NOT_FOUND, got $($nopeParsed.error.code)"
    }

    # 9. Cross-validation: CLI and MCP see the same workspace ids.
    Header "Cross-validation: CLI vs MCP id sets"
    $cliIds = $cliParsed.workspaces | ForEach-Object { $_.id } | Sort-Object
    $mcpIds = $mcpParsed.result.workspaces | ForEach-Object { $_.id } | Sort-Object
    Write-Host "  CLI: $($cliIds -join ', ')"
    Write-Host "  MCP: $($mcpIds -join ', ')"
    if ((Compare-Object $cliIds $mcpIds) -ne $null) {
        throw "CLI and MCP returned different workspace id sets"
    }
    Write-Host "  -> identical" -ForegroundColor Green

    Write-Host ""
    Write-Host "Demo complete — all FleetView surfaces green." -ForegroundColor Green
}
finally {
    # Kill the session orchestrate created so the next run is clean.
    & $psmux kill-session -t $session 2>$null | Out-Null
    if (-not $KeepTemp) {
        Remove-Item -Recurse -Force $workDir -ErrorAction SilentlyContinue
    } else {
        Write-Host ""
        Write-Host "(-KeepTemp) Work dir preserved at: $workDir" -ForegroundColor DarkYellow
    }
}
