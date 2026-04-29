#requires -Version 5.1
<#
Boundary contracts for the v3.4.0-ohboy version mint (sync-2026-04-29-harrier).

We diverged from upstream at 3.3.0 and chose 3.4.0-ohboy to signal the fork
rather than tracking upstream's 3.3.4 patch cadence. Pre-release SemVer
suffix "-ohboy" makes the divergence explicit to anyone running `cargo --version`.

Asserts:
  1. cargo metadata reports package version 3.4.0-ohboy
  2. compiled binary --version reports 3.4.0-ohboy
  3. package name is still "psmux" (no rename regression)
  4. SemVer pre-release suffix "-ohboy" is preserved (not stripped by tooling)
#>

[CmdletBinding()]
param(
    [string]$RepoRoot = 'D:/Home/psmux'
)

$ErrorActionPreference = 'Stop'
$expectedVersion = '3.4.0-ohboy'
$expectedName = 'psmux'
$expectedPrerelease = 'ohboy'

Push-Location $RepoRoot
try {
    # Contract 1: cargo metadata package.version
    $metaJson = cargo metadata --format-version 1 --no-deps 2>$null
    if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed (exit $LASTEXITCODE)" }
    $metaVersion = $metaJson | jq -r '.packages[] | select(.name=="psmux") | .version'
    if ($metaVersion -ne $expectedVersion) {
        throw "Contract 1 FAIL: cargo metadata version is '$metaVersion', expected '$expectedVersion'"
    }
    Write-Host "Contract 1 PASS: cargo metadata reports $metaVersion"

    # Contract 2: compiled binary --version
    cargo build --release --bin psmux 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release --bin psmux failed (exit $LASTEXITCODE)" }
    $binaryPath = Join-Path $RepoRoot 'target/release/psmux.exe'
    if (-not (Test-Path $binaryPath)) { throw "binary not found at $binaryPath" }
    $versionOutput = & $binaryPath --version 2>&1 | Out-String
    if ($versionOutput -notmatch [regex]::Escape($expectedVersion)) {
        throw "Contract 2 FAIL: '$binaryPath --version' output '$($versionOutput.Trim())' does not contain '$expectedVersion'"
    }
    Write-Host "Contract 2 PASS: binary --version contains $expectedVersion"

    # Contract 3: package name still "psmux"
    $metaName = $metaJson | jq -r '.packages[] | select(.version=="' + $expectedVersion + '") | .name'
    if ($metaName -ne $expectedName) {
        throw "Contract 3 FAIL: package name is '$metaName', expected '$expectedName' (rename regression?)"
    }
    Write-Host "Contract 3 PASS: package name is $metaName"

    # Contract 4: pre-release suffix preserved (catches accidental strip-to-release)
    if ($metaVersion -notmatch '-ohboy$') {
        throw "Contract 4 FAIL: pre-release suffix '-$expectedPrerelease' was stripped from '$metaVersion'"
    }
    Write-Host "Contract 4 PASS: SemVer pre-release suffix -$expectedPrerelease preserved"

    Write-Host ""
    Write-Host "All 4 boundary contracts passed for v$expectedVersion."
}
finally {
    Pop-Location
}
