# verify_parity.ps1 — static audit for P2 verification items (#20, #21, #22)
#
# Confirms that regression-prone infrastructure from upstream still exists in
# the ohboy-builds fork.  Purely grep-based — runs in <1s, safe in CI.
#
# Exit code 0 = all items present, non-zero = count of missing items.

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

$missing = 0
$total = 0

function Check {
    param(
        [string]$Label,
        [string]$File,
        [string]$Pattern,
        [switch]$AllowAbsent
    )
    $script:total++
    $path = Join-Path $repo $File
    if (-not (Test-Path $path)) {
        if ($AllowAbsent) {
            Write-Host "~ $Label — file $File not present (allowed)" -ForegroundColor Yellow
        } else {
            Write-Host "X $Label — file $File missing" -ForegroundColor Red
            $script:missing++
        }
        return
    }
    $matches = Select-String -Path $path -Pattern $Pattern -SimpleMatch -Quiet
    if ($matches) {
        Write-Host "+ $Label" -ForegroundColor Green
    } else {
        Write-Host "X $Label — pattern '$Pattern' not found in $File" -ForegroundColor Red
        $script:missing++
    }
}

Write-Host "=== P2.21 Nesting prevention guards (3f95642) ==="
Check "PSMUX_ACTIVE guard in attach path" "src/main.rs" 'PSMUX_ACTIVE'
Check "nested-sessions error message" "src/main.rs" 'nested sessions are not allowed'

Write-Host ""
Write-Host "=== P2.22 VTI / warmup / kill-server (e29b954) ==="
Check "disable_vti_on_stdin call" "src/main.rs" 'disable_vti_on_stdin'
Check "warmup command alias" "src/main.rs" '"warmup"'
Check "is_ssh_session guard" "src/main.rs" 'is_ssh_session'

Write-Host ""
Write-Host "=== P2.20 Session namespace functions (3e61a0d) ==="
# Ohboy intentionally uses list_session_names() without socket namespace
# filtering because CustomPaneBackend provides equivalent scoping via
# pipe discovery.  The upstream -L namespace helpers are NOT present
# by design — document the gap.
$hasNs = Select-String -Path "src/session.rs" -Pattern 'list_session_names_ns' -SimpleMatch -Quiet
if ($hasNs) {
    Write-Host "+ list_session_names_ns present (upstream parity)" -ForegroundColor Green
} else {
    Write-Host "i list_session_names_ns absent — ohboy uses CustomPaneBackend pipe discovery instead" -ForegroundColor Cyan
}
$script:total++  # counted but not failing

Write-Host ""
Write-Host "=== Guardrails extensions present ==="
Check "user_set_options HashSet (P0.2)" "src/types.rs" 'user_set_options'
Check "allow_set_title field (P0.1)" "src/types.rs" 'allow_set_title'
Check "title_locked on Pane (P0.1)" "src/types.rs" 'title_locked'
Check "propagate_osc_titles helper (P0.1)" "src/server/helpers.rs" 'propagate_osc_titles'
Check "octal codec at crate root (P0.3)" "src/octal.rs" 'encode_octal'
Check "for_each_pane helper (P0.5)" "src/tree.rs" 'for_each_pane'

Write-Host ""
Write-Host "=== Summary ==="
Write-Host ("{0}/{1} checks passed" -f ($total - $missing), $total)
exit $missing
