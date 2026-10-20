# Rails bench runner — invokes each test_rails_*.ps1 file, aggregates results,
# exits with the total failure count.  Safe to wire into CI.
#
# Usage:
#   pwsh tests/run_rails_bench.ps1
#   pwsh tests/run_rails_bench.ps1 -Filter wait_for    # run only matching files
#   pwsh tests/run_rails_bench.ps1 -Verbose            # show individual test output

param(
    [string]$Filter = "*",
    [switch]$StopOnFailure,
    [int]$FileTimeoutSec = 300,
    [string]$LogDir = (Join-Path $PSScriptRoot "bench-logs")
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

New-Item -ItemType Directory -Path $LogDir -Force | Out-Null
$startedAt = Get-Date
$totalPass = 0
$totalFail = 0
$fileResults = @()

Write-Host "=== psmux rails-bench ($($files.Count) file(s)) ===" -ForegroundColor Cyan
Write-Host ""

foreach ($f in $files) {
    Write-Host "--- $($f.Name) ---" -ForegroundColor Magenta
    $pre = Get-Date
    # A hung file must fail by name instead of stalling the whole bench (and
    # the CI job, whose log is lost when it is cancelled).
    # Output goes to files, not the CI step's pipe: a psmux server a test leaves
    # running would otherwise inherit the pipe and keep the step open forever.
    $out = Join-Path $LogDir "$($f.BaseName).out.log"
    $err = Join-Path $LogDir "$($f.BaseName).err.log"
    $proc = Start-Process pwsh -ArgumentList '-NoProfile', '-File', "`"$($f.FullName)`"" -NoNewWindow -PassThru `
        -RedirectStandardOutput $out -RedirectStandardError $err
    if ($proc.WaitForExit($FileTimeoutSec * 1000)) {
        $exit = $proc.ExitCode
    } else {
        Write-Host "  X TIMEOUT: $($f.Name) still running after $FileTimeoutSec s, killing its process tree" -ForegroundColor Red
        taskkill /T /F /PID $proc.Id | Out-Null
        $exit = -1
    }
    Get-Content $out, $err -ErrorAction SilentlyContinue | Out-Host
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

# Report and stop any psmux process the tests left behind, so it cannot hold the step open.
$leftover = @(Get-Process -Name psmux* -ErrorAction SilentlyContinue)
Write-Host "Leftover psmux processes after the bench: $($leftover.Count)"
$leftover | Format-Table Id, ProcessName, StartTime -AutoSize | Out-Host
$leftover | Stop-Process -Force -ErrorAction SilentlyContinue

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
