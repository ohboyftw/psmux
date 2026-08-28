# Rails bench: refresh-client rejects control-only flags on the CLI path.
#
# tmux restricts -C/-B/-A/-f to control-mode clients. psmux's one-shot CLI path
# dropped them client-side and the server never saw them, so a caller asking to
# set a size or take out a subscription got exit 0 and no effect. The two halves
# are atomic: forwarding the flags without the server rejection just moves the
# silence, and rejecting server-side without forwarding is never reached.
. $PSScriptRoot/_harness.ps1

Write-Host "== refresh-client control-only flags ==" -ForegroundColor Cyan
$S = New-IsolatedSession "refresh-flags"
Start-Sleep -Milliseconds 400

foreach ($flag in @('-C', '-B', '-A', '-f')) {
    Test-Case "refresh-client $flag is rejected, not silently ignored" {
        $out = psmux refresh-client -t $S $flag '100x40' 2>&1
        $rc = $LASTEXITCODE
        $said = ($out -join ' ') -match 'not a control client'
        if (-not ($rc -ne 0 -and $said)) {
            Write-Host "    rc=$rc out=[$($out -join ' ')]" -ForegroundColor DarkYellow
        }
        ($rc -ne 0) -and $said
    }
}

Test-Case "refresh-client with no control-only flag still succeeds" {
    $null = psmux refresh-client -t $S 2>&1
    $LASTEXITCODE -eq 0
}

Test-Case "refresh-client -S still succeeds" {
    $null = psmux refresh-client -t $S -S 2>&1
    $LASTEXITCODE -eq 0
}

Remove-PsmuxSession $S
Write-Summary
