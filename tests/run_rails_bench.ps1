# Rails bench runner — invokes each test_rails_*.ps1 file, aggregates results,
# exits with the total failure count.  Safe to wire into CI.
#
# Usage:
#   pwsh tests/run_rails_bench.ps1
#   pwsh tests/run_rails_bench.ps1 -Filter wait_for    # run only matching files
#   pwsh tests/run_rails_bench.ps1 -Verbose            # show individual test output

param(
    [string]$Filter = "*",
    [switch]$StopOnFailure
)

$ErrorActionPreference = 'Continue'
$testsDir = $PSScriptRoot
$files = Get-ChildItem -Path $testsDir -Filter "test_rails_*.ps1" |
    Where-Object { $_.Name -like "*$Filter*" } |
    Sort-Object Name

if (-not $files) {
    Write-Host "No test_rails_*.ps1 files matching filter '$Filter'" -ForegroundColor Yellow
    exit 0
}

$startedAt = Get-Date
$totalPass = 0
$totalFail = 0
$fileResults = @()

Write-Host "=== psmux rails-bench ($($files.Count) file(s)) ===" -ForegroundColor Cyan
Write-Host ""

foreach ($f in $files) {
    Write-Host "--- $($f.Name) ---" -ForegroundColor Magenta
    $pre = Get-Date
    & pwsh -NoProfile -File $f.FullName
    $exit = $LASTEXITCODE
    $elapsed = [int]((Get-Date) - $pre).TotalMilliseconds
    $fileResults += [PSCustomObject]@{
        File = $f.Name
        Fail = $exit
        Ms   = $elapsed
    }
    if ($exit -eq 0) {
        $totalPass++
    } else {
        $totalFail++
        if ($StopOnFailure) {
            Write-Host "--- stop on failure triggered ---" -ForegroundColor Red
            break
        }
    }
    Write-Host ""
}

$duration = [int]((Get-Date) - $startedAt).TotalSeconds

Write-Host "=== rails-bench summary ===" -ForegroundColor Cyan
$fileResults | Format-Table File, Fail, Ms -AutoSize | Out-Host
Write-Host ("Files passed: {0}  failed: {1}  elapsed: {2}s" -f $totalPass, $totalFail, $duration)

if ($totalFail -eq 0) {
    Write-Host "ALL GREEN" -ForegroundColor Green
    exit 0
} else {
    Write-Host "FAILURES: $totalFail file(s) had failing cases" -ForegroundColor Red
    exit $totalFail
}
