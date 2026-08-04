# Rails bench: per-server state-file lifecycle contract (#496 / dbfa42d / 63e31ba)
#
# The contract, in one line: while a `.port` exists its `.key` exists, is
# non-empty, and actually authenticates — and when the server is gone, every
# file goes with it.
#
# run_server used to write `.port` (the readiness beacon) BEFORE `.key`, and
# then re-truncate the key after the beacon was already visible, so a client
# that saw `.port` could read an empty credential and fail AUTH.  Nothing
# guarded that ordering: the unit tests cover the sweep, not the server.
. $PSScriptRoot/_harness.ps1

Write-Host "== state-file lifecycle ==" -ForegroundColor Cyan

$psmuxDir = Join-Path $env:USERPROFILE ".psmux"
$S = New-IsolatedSession "statefile"
$ghost = "ghost-" + [guid]::NewGuid().Guid.Substring(0, 8)
$garbage = "garbage-" + [guid]::NewGuid().Guid.Substring(0, 8)

function Get-StateFile { param($Base, $Ext) Join-Path $psmuxDir "$Base.$Ext" }

try {
    Test-Case "a live server has all four state files" {
        $missing = @('port', 'key', 'version', 'pipe') |
            Where-Object { -not (Test-Path (Get-StateFile $S $_)) }
        -not $missing
    }

    Test-Case "the credential is non-empty whenever the beacon exists" {
        (Test-Path (Get-StateFile $S 'port')) -and
        ((Get-Item (Get-StateFile $S 'key')).Length -gt 0)
    }

    Test-Case "the credential on disk actually authenticates" {
        # list-windows is an authenticated command, so an empty or wrong .key
        # is rejected by connection.rs — which is exactly how the beacon-first
        # ordering broke a session.
        #
        # Assert on OUTPUT, never on the exit code: verified by mutation, a
        # one-shot CLI command whose AUTH is rejected prints the server's
        # "ERROR: Authentication required" / "ERROR: Invalid session key" to
        # stdout and still exits 0.  Only the window-listing shape separates
        # success from rejection here.
        $out = psmux list-windows -t $S 2>&1
        $out -match '\d+:'
    }

    Test-Case "the version stamp on disk matches what the binary reports" {
        # The warm-claim path skips a server whose .version differs from this
        # build, but treats a MISSING .version as "legacy, allow" (main.rs).
        # So a stamp that is absent or shaped differently than the reader
        # expects does not fail loudly — it silently claims a stale server.
        # Contract: while a .port exists, .version exists and is `VERSION-HASH`.
        $v = psmux -V 2>&1                       # "psmux 3.4.0-ohboy (63e31ba)"
        if ($v -notmatch '^\S+\s+(\S+)\s+\(([0-9a-f]+)\)') { return $false }
        $expected = "{0}-{1}" -f $Matches[1], $Matches[2]
        $onDisk = (Get-Content -Raw (Get-StateFile $S 'version')).Trim()
        $onDisk -eq $expected
    }

    Test-Case "the sweep spares a live server across repeated invocations" {
        # cleanup_stale_port_files runs on EVERY psmux invocation, so three
        # commands are three chances to reap a server that is still running.
        1..3 | ForEach-Object { $null = psmux list-sessions 2>&1 }
        $missing = @('port', 'key', 'version', 'pipe') |
            Where-Object { -not (Test-Path (Get-StateFile $S $_)) }
        (-not $missing) -and ((Get-Item (Get-StateFile $S 'key')).Length -gt 0)
    }

    Test-Case "the live server still authenticates after those sweeps" {
        (psmux list-windows -t $S 2>&1) -match '\d+:'
    }

    Test-Case "a hard-killed server's whole file set is reclaimed" {
        # Port 0 names no listener, so the bind probe reports it dead.  This is
        # the shape a killed server leaves behind: it removes nothing itself.
        # Explicit -Path/-Value: a positional pipe name like \\.\pipe\x is
        # parsed as a path by Set-Content and the file is never written, which
        # made this assertion pass vacuously.
        Set-Content -Path (Get-StateFile $ghost 'port') -Value '0' -NoNewline
        Set-Content -Path (Get-StateFile $ghost 'key') -Value 'deadbeef' -NoNewline
        Set-Content -Path (Get-StateFile $ghost 'version') -Value '3.4.0' -NoNewline
        Set-Content -Path (Get-StateFile $ghost 'pipe') -Value '\\.\pipe\psmux-ghost' -NoNewline
        $seeded = @('port', 'key', 'version', 'pipe') |
            Where-Object { Test-Path (Get-StateFile $ghost $_) }
        if ($seeded.Count -ne 4) { return $false }  # never assert on an empty fixture
        $null = psmux list-sessions 2>&1
        $left = @('port', 'key', 'version', 'pipe') |
            Where-Object { Test-Path (Get-StateFile $ghost $_) }
        -not $left
    }

    Test-Case "an unparseable beacon is reclaimed, not treated as live" {
        Set-Content -Path (Get-StateFile $garbage 'port') -Value 'not-a-port' -NoNewline
        Set-Content -Path (Get-StateFile $garbage 'key') -Value 'deadbeef' -NoNewline
        if (-not (Test-Path (Get-StateFile $garbage 'port'))) { return $false }
        $null = psmux list-sessions 2>&1
        (-not (Test-Path (Get-StateFile $garbage 'port'))) -and
        (-not (Test-Path (Get-StateFile $garbage 'key')))
    }

    Test-Case "killing the session leaves no state files behind" {
        Remove-PsmuxSession $S
        Start-Sleep -Milliseconds 500
        $null = psmux list-sessions 2>&1
        $left = @('port', 'key', 'version', 'pipe') |
            Where-Object { Test-Path (Get-StateFile $S $_) }
        -not $left
    }
} finally {
    Remove-PsmuxSession $S
    foreach ($base in @($ghost, $garbage)) {
        foreach ($ext in @('port', 'key', 'version', 'pipe')) {
            Remove-Item (Get-StateFile $base $ext) -ErrorAction SilentlyContinue
        }
    }
}

Write-Summary
