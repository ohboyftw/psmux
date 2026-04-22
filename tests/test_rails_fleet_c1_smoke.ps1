# Rails bench: Fleet C1 smoke test — verifies spec §10 success criterion.
#
# Exercises both surfaces against crates/psmux-fleet/fixtures/state_healthy.json
# and asserts that `psmux fleet ls --json` and `psmux-fleet-mcp fleet.list`
# return the same workspace set (3 workspaces).
#
# Requires `cargo build --workspace` to have populated target/debug.

. $PSScriptRoot/_harness.ps1

$repoRoot = Split-Path -Parent $PSScriptRoot
$psmux    = Join-Path $repoRoot 'target\debug\psmux.exe'
$mcp      = Join-Path $repoRoot 'target\debug\psmux-fleet-mcp.exe'
$fixture  = Join-Path $repoRoot 'crates\psmux-fleet\fixtures\state_healthy.json'

Write-Host "== fleet C1 smoke ==" -ForegroundColor Cyan

Test-Case "debug binaries exist (psmux.exe, psmux-fleet-mcp.exe)" {
    (Test-Path $psmux) -and (Test-Path $mcp) -and (Test-Path $fixture)
}

# Bail out early if prerequisites missing — remaining cases would be noise.
if (-not ((Test-Path $psmux) -and (Test-Path $mcp) -and (Test-Path $fixture))) {
    Write-Host "  skipping remaining cases: run ``cargo build --workspace`` first" -ForegroundColor Yellow
    Write-Summary
}

$script:cliParsed = $null
$script:mcpParsed = $null

Test-Case "psmux fleet ls --json reports total=3 and 3 workspaces" {
    $raw = & $psmux fleet ls --state-path $fixture --json 2>&1
    if ($LASTEXITCODE -ne 0) {
        Write-Host "    psmux exited $LASTEXITCODE; output: $($raw | Out-String)" -ForegroundColor DarkYellow
        return $false
    }
    $script:cliParsed = ($raw | Out-String) | ConvertFrom-Json
    ($script:cliParsed.total -eq 3) -and ($script:cliParsed.workspaces.Count -eq 3)
}

Test-Case "psmux-fleet-mcp fleet.list reports result.total=3 and 3 workspaces" {
    $req = '{"jsonrpc":"2.0","id":1,"method":"fleet.list","params":{}}'
    $resp = $req | & $mcp --state-path $fixture 2>$null | Select-Object -First 1
    if (-not $resp) { return $false }
    $script:mcpParsed = $resp | ConvertFrom-Json
    ($script:mcpParsed.result.total -eq 3) -and ($script:mcpParsed.result.workspaces.Count -eq 3)
}

Test-Case "CLI and MCP return the same workspace id set" {
    if (-not $script:cliParsed -or -not $script:mcpParsed) { return $false }
    $cliIds = @($script:cliParsed.workspaces | ForEach-Object { $_.id } | Sort-Object)
    $mcpIds = @($script:mcpParsed.result.workspaces | ForEach-Object { $_.id } | Sort-Object)
    $diff = Compare-Object $cliIds $mcpIds
    ($null -eq $diff) -and ($cliIds.Count -eq 3)
}

Write-Summary
