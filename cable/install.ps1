# Install psmux cable channels for television (tv)
# Usage: pwsh cable/install.ps1

$cableDir = "$env:LOCALAPPDATA\television\config\cable"
if (-not (Test-Path $cableDir)) {
    New-Item -ItemType Directory -Path $cableDir -Force | Out-Null
}

$channels = Get-ChildItem "$PSScriptRoot\*.toml"
foreach ($ch in $channels) {
    Copy-Item $ch.FullName "$cableDir\$($ch.Name)" -Force
    Write-Host "Installed: $($ch.BaseName)"
}

Write-Host "`n$($channels.Count) psmux channels installed to $cableDir"
Write-Host "Try: tv psmux-panes"
