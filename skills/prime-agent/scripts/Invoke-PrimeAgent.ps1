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

if ($LoadFunctionsOnly) { return }
