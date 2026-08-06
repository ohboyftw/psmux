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

Describe 'Test-KernelHealth' {
    It 'reports unhealthy when the interpreter does not exist' {
        $r = Test-KernelHealth -PythonPath 'C:\definitely\not\here\python.exe'
        $r.Healthy | Should -BeFalse
        $r.Detail  | Should -Not -BeNullOrEmpty
    }

    It 'reports unhealthy for a bare venv with no kernel packages' {
        $tmp = Join-Path $env:TEMP "pa-health-$([guid]::NewGuid().Guid.Substring(0,8))"
        try {
            uv venv $tmp --python 3.11 --seed *> $null
            $r = Test-KernelHealth -PythonPath (Join-Path $tmp 'Scripts\python.exe')
            $r.Healthy | Should -BeFalse
            $r.Detail  | Should -Match 'ModuleNotFoundError'
        } finally { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
    }

    It 'reports healthy for the live kernel venv' {
        $r = Test-KernelHealth -PythonPath (Join-Path $HOME '.prime\agent\kernel-venv\Scripts\python.exe')
        $r.Healthy | Should -BeTrue
    }
}

Describe 'Repair-KernelVenv' -Tag 'Slow' {
    It 'turns a bare venv into a healthy kernel venv' {
        $tmp = Join-Path $env:TEMP "pa-repair-$([guid]::NewGuid().Guid.Substring(0,8))"
        try {
            uv venv $tmp --python 3.11 --seed *> $null
            (Test-KernelHealth -PythonPath (Join-Path $tmp 'Scripts\python.exe')).Healthy |
                Should -BeFalse

            $r = Repair-KernelVenv -VenvRoot $tmp
            $r.Succeeded | Should -BeTrue
            (Test-KernelHealth -PythonPath (Join-Path $tmp 'Scripts\python.exe')).Healthy |
                Should -BeTrue
        } finally { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
    }

    It 'reports failure without throwing when the runtime source is missing' {
        $tmp = Join-Path $env:TEMP "pa-norun-$([guid]::NewGuid().Guid.Substring(0,8))"
        $saved = $env:APPDATA
        try {
            $env:APPDATA = $tmp
            $r = Repair-KernelVenv -VenvRoot $tmp
            $r.Succeeded | Should -BeFalse
            $r.Detail    | Should -Not -BeNullOrEmpty
        } finally {
            $env:APPDATA = $saved
            Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
        }
    }
}
