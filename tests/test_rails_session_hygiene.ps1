# Rails bench: session hygiene — namespaced resurrectable snapshots, no stray
# " \x08" in a command pane's first stdin bytes.
#
# Two contracts, from two findings on ohboy-builds 727a8ab5:
#
#  1. A resurrection snapshot belongs to the server/namespace that wrote it.
#     `list-sessions` on a brand-new isolated `-L` server printed 48 dead
#     sessions like `ps-126988: (resurrectable)` — snapshots left behind by
#     earlier test runs and other servers' namespaces. A snapshot must only be
#     listed under the `-L` that owns it, and a deliberately killed session
#     must not leave a snapshot behind at all (otherwise every bench run, each
#     with a fresh unique session name, strands another `(resurrectable)`
#     entry forever).
#
#  2. A pane spawned with a command owns its stdin from the very first byte.
#     psmux injects a Space+Backspace primer (0x20 0x08) before the first
#     send-keys payload to absorb a ConPTY/PSReadLine race — a real workaround
#     for interactive shells, but a raw-mode program (this probe) receives it
#     as actual keystrokes. The primer must never reach a command pane.
. $PSScriptRoot/_harness.ps1

Write-Host "== session hygiene ==" -ForegroundColor Cyan

# ── 1. snapshots: namespace scoping and kill-session cleanup ────────────────
# An isolated data dir keeps the resurrect directory deterministic.

$dataDir = Join-Path $env:TEMP "psmux-hygiene-$([guid]::NewGuid().Guid.Substring(0,8))"
New-Item -ItemType Directory -Path $dataDir -Force | Out-Null
$keepDataDir = $env:PSMUX_DATA_DIR
$env:PSMUX_DATA_DIR = $dataDir
$probe = $null
$resurrectDir = Join-Path $dataDir "resurrect"

function Get-Snapshot { param([string]$Stem) Join-Path $resurrectDir "$Stem.json" }
function Plant-Snapshot {
    param([string]$Stem)
    New-Item -ItemType Directory -Path $resurrectDir -Force | Out-Null
    # A single quoted token: the listing only checks the .json suffix, and
    # delete must remove exactly this file.
    Set-Content -Path (Get-Snapshot $Stem) -Value '{"version":1}'
}

try {
    $S = "hygiene-" + [guid]::NewGuid().Guid.Substring(0,8)
    $null = psmux new-session -d -s $S 2>&1
    Start-Sleep -Milliseconds 600
    # A structural change (split) is what saves a snapshot on the server.
    $null = psmux split-window -t $S 2>&1
    Start-Sleep -Milliseconds 1000

    Test-Case "a structural change saves a snapshot under the session stem" {
        Test-Path (Get-Snapshot $S)
    }

    $null = psmux kill-session -t $S 2>&1
    Start-Sleep -Milliseconds 1000

    Test-Case "kill-session deletes the session's snapshot" {
        if (Test-Path (Get-Snapshot $S)) {
            Write-Host "    still present: $(Get-Snapshot $S)" -ForegroundColor DarkYellow
        }
        -not (Test-Path (Get-Snapshot $S))
    }

    Test-Case "a killed session is not listed as resurrectable" {
        $out = (psmux list-sessions 2>&1) -join "`n"
        -not ($out -match 'resurrectable')
    }

    # Namespace scoping, against planted stems: a snapshot file is only
    # visible to the namespace whose prefix it carries.
    $ns = "hygns-" + [guid]::NewGuid().Guid.Substring(0,8)
    $nsStem = "${ns}__victim"
    $plainStem = "hygplain-" + [guid]::NewGuid().Guid.Substring(0,8)
    Plant-Snapshot $nsStem
    Plant-Snapshot $plainStem

    Test-Case "list-sessions without -L hides namespaced snapshots" {
        $out = (psmux list-sessions 2>&1) -join "`n"
        ($out -match [regex]::Escape("$plainStem" + ": (resurrectable)")) -and
        (-not ($out -match [regex]::Escape($nsStem)))
    }

    Test-Case "list-sessions -L ns shows only that namespace's snapshots" {
        $out = (psmux -L $ns list-sessions 2>&1) -join "`n"
        ($out -match [regex]::Escape("$nsStem" + ": (resurrectable)")) -and
        (-not ($out -match [regex]::Escape($plainStem)))
    }

    Test-Case "delete-resurrect --all without -L leaves other namespaces alone" {
        $null = psmux delete-resurrect --all 2>&1
        (-not (Test-Path (Get-Snapshot $plainStem))) -and
        (Test-Path (Get-Snapshot $nsStem))
    }

    Test-Case "delete-resurrect --all under -L clears only that namespace" {
        $null = psmux -L $ns delete-resurrect --all 2>&1
        -not (Test-Path (Get-Snapshot $nsStem))
    }

    # ── 2. the Space+Backspace primer must not reach a command pane ─────────
    # The probe reads the first stdin bytes it is given and prints their hex.
    # If the primer leaked, the first bytes are 20 08 (space, backspace)
    # instead of the payload 48 45 4c 4f 58 ('HELOX').

    $P = "hygprimer-" + [guid]::NewGuid().Guid.Substring(0,8)
    $probe = Join-Path $env:TEMP "psmux-primer-probe-$([guid]::NewGuid().Guid.Substring(0,8)).ps1"
    @'
$buf = New-Object byte[] 16
$n = [System.Console]::OpenStandardInput().Read($buf, 0, $buf.Length)
if ($n -gt 0) {
    Write-Output ("FIRSTBYTES: " + (($buf[0..($n-1)] | ForEach-Object { $_.ToString('x2') }) -join ' '))
}
'@ | Set-Content -LiteralPath $probe -Encoding ascii
    $null = psmux new-session -d -s $P -- pwsh -NoProfile -File $probe 2>&1
    Start-Sleep -Milliseconds 1000
    $null = psmux send-keys -t $P -l 'HELOX' 2>&1
    $null = psmux send-keys -t $P Enter 2>&1
    $seen = Wait-ForOutput -Target $P -Pattern 'FIRSTBYTES' -TimeoutMs 15000

    Test-Case "a command pane's first stdin bytes are the payload, not '20 08'" {
        if (-not $seen) {
            Write-Host "    probe never printed FIRSTBYTES" -ForegroundColor DarkYellow
            return $false
        }
        $out = (psmux capture-pane -p -t $P 2>$null) -join "`n"
        if ($out -notmatch 'FIRSTBYTES: 48 45 4c 4f 58') {
            Write-Host "    pane=[$($out -replace "`r?`n", ' ')]" -ForegroundColor DarkYellow
        }
        ($out -match 'FIRSTBYTES: 48 45 4c 4f 58') -and (-not ($out -match '20 08'))
    }

    Remove-PsmuxSession $P
} finally {
    $env:PSMUX_DATA_DIR = $keepDataDir
    Remove-Item -LiteralPath $dataDir -Recurse -Force -ErrorAction SilentlyContinue
    if ($probe) { Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue }
}

Write-Summary