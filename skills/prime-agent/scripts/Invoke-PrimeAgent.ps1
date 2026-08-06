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

if ($LoadFunctionsOnly) { return }
