# zoxide-pick.ps1 — Zoxide directory picker for psmux popup
# Usage: psmux display-popup -E -w 90 -h 20 "pwsh -NoProfile -File path/to/zoxide-pick.ps1"

$ErrorActionPreference = "SilentlyContinue"

# Find binaries
$zoxide = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages\*zoxide*\zoxide.exe" -Recurse | Select-Object -First 1
if (-not $zoxide) { $zoxide = Get-Command zoxide -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source }

$fzf = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages\*fzf*\fzf.exe" -Recurse | Select-Object -First 1
if (-not $fzf) { $fzf = Get-Command fzf -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source }

if (-not $zoxide) { Write-Host "zoxide not found. Install: winget install ajeetdsouza.zoxide" -ForegroundColor Red; Read-Host; exit 1 }
if (-not $fzf) { Write-Host "fzf not found. Install: winget install junegunn.fzf" -ForegroundColor Red; Read-Host; exit 1 }

# Query zoxide, strip scores, pipe to fzf
$dirs = & $zoxide query -ls | ForEach-Object { ($_ -replace '^\s*[\d.]+\s+', '').Trim() } | Where-Object { $_ -ne '' }

if (-not $dirs) {
    Write-Host "No zoxide entries yet. Use 'cd' to build history." -ForegroundColor Yellow
    Read-Host
    exit 0
}

$selected = $dirs | & $fzf --height=100% --layout=reverse --prompt="cd> " --no-sort

if ($selected) {
    # Open a new pane at the selected directory
    $selected = $selected.Trim()
    & psmux split-window -h -c $selected
}
