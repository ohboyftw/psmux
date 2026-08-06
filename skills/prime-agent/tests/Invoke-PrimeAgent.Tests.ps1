BeforeAll {
    $script:ScriptPath = Join-Path $PSScriptRoot '..\scripts\Invoke-PrimeAgent.ps1'
    . $script:ScriptPath -LoadFunctionsOnly
}

Describe 'Resolve-KernelPython' {
    It 'returns PRIME_AGENT_KERNEL_PYTHON when it is set' {
        $env:PRIME_AGENT_KERNEL_PYTHON = 'C:\somewhere\python.exe'
        try { Resolve-KernelPython | Should -Be 'C:\somewhere\python.exe' }
        finally { Remove-Item Env:\PRIME_AGENT_KERNEL_PYTHON -ErrorAction SilentlyContinue }
    }

    It 'falls back to the default kernel-venv path when the variable is absent' {
        Remove-Item Env:\PRIME_AGENT_KERNEL_PYTHON -ErrorAction SilentlyContinue
        Resolve-KernelPython | Should -Be (Join-Path $HOME '.prime\agent\kernel-venv\Scripts\python.exe')
    }

    It 'treats an empty variable as unset' {
        $env:PRIME_AGENT_KERNEL_PYTHON = ''
        try { Resolve-KernelPython | Should -Be (Join-Path $HOME '.prime\agent\kernel-venv\Scripts\python.exe') }
        finally { Remove-Item Env:\PRIME_AGENT_KERNEL_PYTHON -ErrorAction SilentlyContinue }
    }
}
