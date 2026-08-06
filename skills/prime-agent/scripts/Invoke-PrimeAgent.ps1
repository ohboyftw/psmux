#Requires -Version 7.0
[CmdletBinding()]
param(
    [string]$Prompt,
    [string]$Cwd = (Get-Location).Path,
    [int]$TimeoutSec = 900,
    [switch]$NoRepair,
    [switch]$Json,
    [switch]$LoadFunctionsOnly
)

Set-StrictMode -Version Latest

$script:SkillSocket = '\\.\pipe\prime-agent-cc-skill'

function Resolve-KernelPython {
    [OutputType([string])]
    param()
    $override = ($env:PRIME_AGENT_KERNEL_PYTHON ?? '').Trim()
    if ($override) { return $override }
    return (Join-Path $HOME '.prime\agent\kernel-venv\Scripts\python.exe')
}

$script:ProbeModules = @(
    'ipykernel','dill',
    'requests','httpx','yaml','tomli','dotenv','pandas','numpy','scipy',
    'bs4','lxml','pydantic','tyro',
    'agent_message','agent_observe','attach_image','compact','edit','goal',
    'rlm_heartbeat','websearch'
)

function Test-KernelHealth {
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$PythonPath)

    if (-not (Test-Path -LiteralPath $PythonPath)) {
        return [pscustomobject]@{ Healthy = $false; Detail = "interpreter not found: $PythonPath" }
    }
    $code = "import $($script:ProbeModules -join ', ')" +
            "`nimport rlm`nassert callable(rlm.harness.create_skill)"
    $err = & $PythonPath -c $code 2>&1
    if ($LASTEXITCODE -eq 0) {
        return [pscustomobject]@{ Healthy = $true; Detail = '' }
    }
    return [pscustomobject]@{ Healthy = $false; Detail = ($err | Out-String).Trim() }
}

function Get-PrimeAgentDist {
    [OutputType([string])]
    param()
    return (Join-Path $env:APPDATA 'npm\node_modules\prime-agent\dist')
}

function Get-RepairArgs {
    [OutputType([string[]])]
    param([Parameter(Mandatory)][string]$Dist)

    $a = @((Join-Path $Dist 'prime-agent-runtime'),
           'ipykernel','dill','requests','httpx','pyyaml','tomli','python-dotenv',
           'pandas','numpy','scipy','beautifulsoup4','lxml','pydantic','tyro')
    foreach ($s in @('agent-message','agent-observe','attach-image','compact',
                     'edit','goal','rlm-heartbeat','websearch')) {
        $a += '--editable'; $a += (Join-Path $Dist "skills\$s")
    }
    return $a
}

function Repair-KernelVenv {
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$VenvRoot)

    $dist = Get-PrimeAgentDist
    $runtime = Join-Path $dist 'prime-agent-runtime'
    if (-not (Test-Path -LiteralPath (Join-Path $runtime 'pyproject.toml'))) {
        return [pscustomobject]@{ Succeeded = $false; Detail = "runtime source not found: $runtime" }
    }

    Remove-Item -Recurse -Force $VenvRoot -ErrorAction SilentlyContinue
    $create = uv venv $VenvRoot --python 3.11 --seed 2>&1
    if ($LASTEXITCODE -ne 0) {
        return [pscustomobject]@{ Succeeded = $false; Detail = ($create | Out-String).Trim() }
    }

    $out = uv pip install --python (Join-Path $VenvRoot 'Scripts\python.exe') @(Get-RepairArgs $dist) 2>&1
    if ($LASTEXITCODE -ne 0) {
        return [pscustomobject]@{ Succeeded = $false; Detail = ($out | Out-String).Trim() }
    }
    return [pscustomobject]@{ Succeeded = $true; Detail = '' }
}

function Stop-SkillDaemon {
    param([string]$SocketName = $script:SkillSocket)
    $leaf = Split-Path $SocketName -Leaf
    Get-CimInstance Win32_Process -Filter "Name='node.exe'" |
        Where-Object { $_.CommandLine -and $_.CommandLine.Contains($leaf) } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}

function Get-DelegationExitCode {
    [OutputType([int])]
    param(
        [bool]$PrimeAgentOnPath, [bool]$Healthy, [bool]$RepairSuppressed,
        [bool]$RepairSucceeded, [bool]$TimedOut, [int]$ProcessExitCode
    )
    if (-not $PrimeAgentOnPath) { return 2 }
    if (-not $Healthy) {
        if ($RepairSuppressed)   { return 3 }
        if (-not $RepairSucceeded) { return 4 }
    }
    if ($TimedOut) { return 5 }
    if ($ProcessExitCode -ne 0) { return 6 }
    return 0
}

function Test-ShouldRepairVenv {
    [OutputType([bool])]
    param([Parameter(Mandatory)][string]$PythonPath)
    # Only the default kernel-venv location is safe to auto-rebuild. An
    # overridden interpreter (PRIME_AGENT_KERNEL_PYTHON) may point anywhere,
    # including a stale path never meant to be destroyed.
    return $PythonPath -eq (Join-Path $HOME '.prime\agent\kernel-venv\Scripts\python.exe')
}

if ($LoadFunctionsOnly) { return }

if (-not $Prompt) { Write-Error 'Prompt is required.'; exit 6 }

$onPath = [bool](Get-Command prime-agent -ErrorAction SilentlyContinue)
$python = Resolve-KernelPython
$health = if ($onPath) { Test-KernelHealth -PythonPath $python } else {
    [pscustomobject]@{ Healthy = $false; Detail = 'prime-agent not on PATH' } }
$repaired = $true
$repairSuppressed = [bool]$NoRepair

if ($onPath -and -not $health.Healthy -and -not $NoRepair) {
    if (-not (Test-ShouldRepairVenv -PythonPath $python)) {
        $repairSuppressed = $true
        Write-Error "prime-agent kernel unhealthy at overridden interpreter path; refusing to auto-repair: $python"
    } else {
        Write-Host "prime-agent kernel unhealthy; rebuilding. $($health.Detail)"
        $venvRoot = Split-Path (Split-Path $python -Parent) -Parent
        $result = Repair-KernelVenv -VenvRoot $venvRoot
        $repaired = $result.Succeeded
        if ($repaired) { Stop-SkillDaemon } else { Write-Error $result.Detail }
    }
}

$outFile = New-TemporaryFile
$errFile = New-TemporaryFile
$timedOut = $false
$procExit = 0
$canInvoke = $onPath -and ($health.Healthy -or (-not $repairSuppressed -and $repaired))

if ($canInvoke) {
    $env:PRIME_AGENT_KERNEL_PYTHON = $python
    $agentArgs = @('-p','--no-session','--daemon-socket',$script:SkillSocket,'--cwd',$Cwd)
    if ($Json) { $agentArgs += @('--mode','json') }
    $agentArgs += $Prompt
    $proc = Start-Process -FilePath 'prime-agent.cmd' -ArgumentList $agentArgs -NoNewWindow `
        -PassThru -RedirectStandardOutput $outFile -RedirectStandardError $errFile
    try {
        Wait-Process -Id $proc.Id -Timeout $TimeoutSec -ErrorAction Stop
        $procExit = $proc.ExitCode
    } catch [System.TimeoutException] {
        $timedOut = $true
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    }
    Get-Content $outFile -Raw -ErrorAction SilentlyContinue
    if ($procExit -ne 0) { Get-Content $errFile -Raw -ErrorAction SilentlyContinue | Write-Error }
}

Remove-Item $outFile, $errFile -Force -ErrorAction SilentlyContinue
exit (Get-DelegationExitCode -PrimeAgentOnPath $onPath -Healthy $health.Healthy `
    -RepairSuppressed $repairSuppressed -RepairSucceeded $repaired `
    -TimedOut $timedOut -ProcessExitCode $procExit)
