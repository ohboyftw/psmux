# psmux Power Pack installer for Windows
# Run as: irm https://raw.githubusercontent.com/ohboyftw/psmux/ohboy-builds/scripts/install.ps1 | iex
# Or locally: .\scripts\install.ps1
#
# Installs psmux + optional companion tools:
#   ripgrep, fd, bat, zoxide, fzf, starship, fastfetch
#
# Flags:
#   -Full         Install psmux + all companion tools
#   -ToolsOnly    Install companion tools only (skip psmux binary)
#   -NoTools      Install psmux only (skip companion tools)
#   -Force        Overwrite existing installations

param(
    [string]$InstallDir = "$env:LOCALAPPDATA\psmux",
    [switch]$Full,
    [switch]$ToolsOnly,
    [switch]$NoTools,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

Write-Host ""
Write-Host "  psmux Power Pack" -ForegroundColor Cyan
Write-Host "  tmux for Windows, built for AI agents" -ForegroundColor DarkGray
Write-Host ""

# ── Companion tools (winget-based) ──────────────────────────────────

$CompanionTools = @(
    # Core — everyone gets these
    @{ Name = "ripgrep";   WingetId = "BurntSushi.ripgrep.MSVC"; Cmd = "rg";        Desc = "Fast search (grep replacement)" }
    @{ Name = "fd";        WingetId = "sharkdp.fd";              Cmd = "fd";        Desc = "Fast find (find replacement)" }
    @{ Name = "bat";       WingetId = "sharkdp.bat";             Cmd = "bat";       Desc = "Syntax-highlighted cat" }
    @{ Name = "zoxide";    WingetId = "ajeetdsouza.zoxide";      Cmd = "zoxide";    Desc = "Smart cd (frecency)" }
    @{ Name = "fzf";       WingetId = "junegunn.fzf";            Cmd = "fzf";       Desc = "Fuzzy finder" }
    @{ Name = "starship";  WingetId = "Starship.Starship";       Cmd = "starship";  Desc = "Cross-shell prompt" }
    @{ Name = "fastfetch"; WingetId = "Fastfetch-cli.Fastfetch"; Cmd = "fastfetch"; Desc = "System info splash" }
    # Extras — keep the docs/power-pack-tools.md list aligned with what the installer ships
    @{ Name = "atuin";     WingetId = "Atuinsh.Atuin";           Cmd = "atuin";     Desc = "Cross-shell history with search" }
    @{ Name = "eza";       WingetId = "eza-community.eza";       Cmd = "eza";       Desc = "Modern ls (color + git)" }
    @{ Name = "jq";        WingetId = "jqlang.jq";               Cmd = "jq";        Desc = "JSON query/filter" }
    @{ Name = "ast-grep";  WingetId = "ast-grep.ast-grep";       Cmd = "sg";        Desc = "AST-aware structural search/rewrite" }
    @{ Name = "tokei";     WingetId = "XAMPPRocky.Tokei";        Cmd = "tokei";     Desc = "Fast LOC counter" }
    @{ Name = "gh";        WingetId = "GitHub.cli";              Cmd = "gh";        Desc = "GitHub CLI" }
)

function Install-CompanionTools {
    $hasWinget = Get-Command winget -ErrorAction SilentlyContinue
    if (-not $hasWinget) {
        Write-Host "  winget not found -- skipping companion tools" -ForegroundColor Yellow
        Write-Host "  Install winget from aka.ms/winget then re-run with -Full" -ForegroundColor DarkGray
        return
    }

    Write-Host "Installing companion tools via winget..." -ForegroundColor Cyan
    $installed = 0
    $skipped = 0
    foreach ($tool in $CompanionTools) {
        $exists = Get-Command $tool.Cmd -ErrorAction SilentlyContinue
        if ($exists -and -not $Force) {
            Write-Host "  $($tool.Name) -- already installed" -ForegroundColor DarkGray
            $skipped++
            continue
        }
        Write-Host "  $($tool.Name) -- $($tool.Desc)" -ForegroundColor Green
        try {
            winget install --id $tool.WingetId --accept-source-agreements --accept-package-agreements --silent 2>$null | Out-Null
            $installed++
        } catch {
            Write-Host "    failed: $_" -ForegroundColor Yellow
        }
    }
    Write-Host "  Done: $installed installed, $skipped already present" -ForegroundColor Cyan
    Write-Host ""
}

function Install-ShellIntegration {
    # ── zoxide init ──
    if (Get-Command zoxide -ErrorAction SilentlyContinue) {
        $profilePath = $PROFILE.CurrentUserAllHosts
        if ($profilePath -and (Test-Path $profilePath)) {
            $profileContent = Get-Content $profilePath -Raw -ErrorAction SilentlyContinue
            if ($profileContent -notmatch 'Invoke-Expression.*zoxide init') {
                Write-Host "  Adding zoxide init to PowerShell profile..." -ForegroundColor Green
                Add-Content -Path $profilePath -Value "`n# zoxide smart-cd (added by psmux installer)`nInvoke-Expression (& { (zoxide init powershell | Out-String) })"
            } else {
                Write-Host "  zoxide already in PowerShell profile" -ForegroundColor DarkGray
            }
        }
    }

    # ── starship init ──
    if (Get-Command starship -ErrorAction SilentlyContinue) {
        $profilePath = $PROFILE.CurrentUserAllHosts
        if ($profilePath -and (Test-Path $profilePath)) {
            $profileContent = Get-Content $profilePath -Raw -ErrorAction SilentlyContinue
            if ($profileContent -notmatch 'Invoke-Expression.*starship init') {
                Write-Host "  Adding starship init to PowerShell profile..." -ForegroundColor Green
                Add-Content -Path $profilePath -Value "`n# starship prompt (added by psmux installer)`nInvoke-Expression (& starship init powershell)"
            } else {
                Write-Host "  starship already in PowerShell profile" -ForegroundColor DarkGray
            }
        }
    }
}

# ── psmux binary installation ───────────────────────────────────────

if ($ToolsOnly) {
    Write-Host "Skipping psmux binary (--ToolsOnly mode)" -ForegroundColor DarkGray
} else {

# Determine if we're installing from local build or downloading
# When run via iex, $PSScriptRoot is empty
$LocalBuild = $false
if ($PSScriptRoot -and (Test-Path "$PSScriptRoot\..\target\release\psmux.exe")) {
    $LocalBuild = $true
    $RepoRoot = Split-Path -Parent $PSScriptRoot
}

if ($LocalBuild) {
    Write-Host "Installing from local build..." -ForegroundColor Yellow
    $SourceDir = "$RepoRoot\target\release"
} else {
    Write-Host "Downloading latest release..." -ForegroundColor Yellow
    
    # Detect architecture using PROCESSOR_ARCHITECTURE env var
    # (RuntimeInformation::OSArchitecture returns $null in PS 5.1 when piped via iex)
    $arch = $env:PROCESSOR_ARCHITECTURE
    # WoW64 correction: 32-bit process on 64-bit OS reports x86; use the real OS arch
    if ($arch -eq "x86" -and $env:PROCESSOR_ARCHITEW6432) {
        $arch = $env:PROCESSOR_ARCHITEW6432
    }
    switch ($arch) {
        "AMD64" { $archLabel = "x64";   $assetPattern = "windows-x64" }
        "x86"   { $archLabel = "x86";   $assetPattern = "windows-x86" }
        "ARM64" { $archLabel = "arm64"; $assetPattern = "windows-arm64" }
        default {
            Write-Host "Unsupported architecture: $arch" -ForegroundColor Red
            exit 1
        }
    }
    Write-Host "Detected architecture: $archLabel" -ForegroundColor Cyan
    
    # Get latest release info
    $ReleasesUrl = "https://api.github.com/repos/psmux/psmux/releases/latest"
    try {
        $Release = Invoke-RestMethod -Uri $ReleasesUrl -Headers @{ "User-Agent" = "psmux-installer" }
        $Asset = $Release.assets | Where-Object { $_.name -match "$assetPattern.*zip" } | Select-Object -First 1
        
        # Fallback: if no arch-specific asset, try x64 (Windows on ARM can run x64 via emulation)
        if (-not $Asset -and $archLabel -eq "arm64") {
            Write-Host "No ARM64 build found, falling back to x64 (runs via emulation)..." -ForegroundColor Yellow
            $Asset = $Release.assets | Where-Object { $_.name -match "windows-x64.*zip" } | Select-Object -First 1
        }
        
        if (-not $Asset) {
            throw "No compatible release asset found for $archLabel"
        }
        
        $DownloadUrl = $Asset.browser_download_url
        $TempZip = "$env:TEMP\psmux-download.zip"
        $TempExtract = "$env:TEMP\psmux-extract"
        
        Write-Host "Downloading from: $DownloadUrl"
        Invoke-WebRequest -Uri $DownloadUrl -OutFile $TempZip
        
        # Extract
        if (Test-Path $TempExtract) { Remove-Item -Recurse -Force $TempExtract }
        Expand-Archive -Path $TempZip -DestinationPath $TempExtract -Force
        
        $SourceDir = $TempExtract
        
    } catch {
        Write-Host "Error downloading release: $_" -ForegroundColor Red
        Write-Host "Try installing from a local build instead:" -ForegroundColor Yellow
        Write-Host "  cargo build --release" -ForegroundColor White
        Write-Host "  .\scripts\install.ps1" -ForegroundColor White
        exit 1
    }
}

# Create install directory
if (-not (Test-Path $InstallDir)) {
    Write-Host "Creating install directory: $InstallDir"
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

# Copy binaries
$Binaries = @("psmux.exe", "pmux.exe", "tmux.exe")
foreach ($bin in $Binaries) {
    $src = Join-Path $SourceDir $bin
    $dst = Join-Path $InstallDir $bin

    if (Test-Path $src) {
        Write-Host "  Installing $bin..." -ForegroundColor Green
        Copy-Item -Path $src -Destination $dst -Force
    } else {
        Write-Host "  Warning: $bin not found" -ForegroundColor Yellow
    }
}

# Archive the PDB so future minidumps of psmux can be symbolicated.
# Without this, the PDB lives in target/release/ and gets overwritten on
# every `cargo build --release` — incident forensics against stale running
# processes becomes impossible. See docs/faq.md crash-diagnostics.
$pdbSrc = Join-Path $SourceDir "psmux.pdb"
if (Test-Path $pdbSrc) {
    Copy-Item -Path $pdbSrc -Destination (Join-Path $InstallDir "psmux.pdb") -Force
    Write-Host "  Archived psmux.pdb alongside psmux.exe" -ForegroundColor DarkGray
}

# Add to PATH if not already there
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    Write-Host "Adding to PATH..." -ForegroundColor Green
    $NewPath = "$UserPath;$InstallDir"
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    $env:Path = "$env:Path;$InstallDir"
    Write-Host "  Added $InstallDir to user PATH" -ForegroundColor Green
} else {
    Write-Host "Already in PATH" -ForegroundColor Gray
}

# Cleanup temp files if downloaded
if (-not $LocalBuild) {
    if (Test-Path $TempZip) { Remove-Item $TempZip -Force }
    if (Test-Path $TempExtract) { Remove-Item -Recurse -Force $TempExtract }
}

Write-Host ""
Write-Host "Installation complete!" -ForegroundColor Green
Write-Host ""
Write-Host "You can now use:" -ForegroundColor Cyan
Write-Host "  psmux    - Start/attach to terminal multiplexer"
Write-Host "  pmux     - Alias for psmux"  
Write-Host "  tmux     - tmux-compatible alias"
Write-Host ""
Write-Host "Quick start:" -ForegroundColor Cyan
Write-Host "  psmux                    # Start new session or attach to 'default'"
Write-Host "  psmux new -s mysession   # Create named session"
Write-Host "  psmux ls                 # List sessions"
Write-Host "  psmux attach -t name     # Attach to session"
Write-Host ""
Write-Host "Note: Restart your terminal or run:" -ForegroundColor Yellow
Write-Host '  $env:Path = [Environment]::GetEnvironmentVariable("Path", "User") + ";" + [Environment]::GetEnvironmentVariable("Path", "Machine")'

} # end if (-not $ToolsOnly)

# ── Companion tools ─────────────────────────────────────────────────

if ($Full -or $ToolsOnly) {
    Install-CompanionTools
    Install-ShellIntegration
} elseif (-not $NoTools) {
    Write-Host ""
    Write-Host "Tip: Run with -Full to also install companion tools:" -ForegroundColor DarkGray
    Write-Host "  irm https://raw.githubusercontent.com/ohboyftw/psmux/ohboy-builds/scripts/install.ps1 | iex -Full" -ForegroundColor DarkGray
    Write-Host "  Tools: ripgrep, fd, bat, zoxide, fzf, starship, fastfetch" -ForegroundColor DarkGray
}

# ── Default config ──────────────────────────────────────────────────

$configDir = "$env:USERPROFILE"
$configFile = "$configDir\.psmux.conf"
if (-not (Test-Path $configFile)) {
    Write-Host ""
    Write-Host "Creating default config at $configFile..." -ForegroundColor Green
    @"
# psmux configuration (tmux-compatible syntax)
# See: psmux list-keys, psmux show-options

# Prefix key (default: Ctrl+b)
# set -g prefix C-b

# Enable mouse support
set -g mouse on

# Set default shell (uncomment to change from PowerShell)
# set -g default-shell bash

# Status bar
set -g status-position bottom
set -g status-justify centre

# Zoxide directory picker (requires zoxide + fzf)
bind z run 'pwsh -NoProfile -File "$env:LOCALAPPDATA\psmux\zoxide-pick.ps1"'
"@ | Set-Content -Path $configFile -Encoding UTF8
    Write-Host "  Edit with: notepad $configFile" -ForegroundColor DarkGray
}

Write-Host ""
Write-Host "Done!" -ForegroundColor Green
Write-Host ""
