# Record all psmux demos using PowerSession + agg
#
# Usage: pwsh -NoProfile -File demos/record-all.ps1
#        pwsh -NoProfile -File demos/record-all.ps1 -Demo 2        # record only demo 02
#        pwsh -NoProfile -File demos/record-all.ps1 -GifOnly       # skip recording, just convert
#        pwsh -NoProfile -File demos/record-all.ps1 -Mp4            # also generate MP4 via ffmpeg

param(
    [int]$Demo = 0,          # 0 = all, 1-5 = specific demo
    [switch]$GifOnly,         # skip recording, just re-render GIFs
    [switch]$Mp4,             # also generate MP4 (requires ffmpeg)
    [int]$Cols = 120,
    [int]$Rows = 35,
    [int]$FontSize = 16
)

$ErrorActionPreference = 'Stop'

$demos = @(
    @{ Num = 1; Name = "hero";              Script = "demo-01-hero.ps1";              Desc = "Hero — psmux in 30 seconds" }
    @{ Num = 2; Name = "powerpack-tools";   Script = "demo-02-powerpack-tools.ps1";   Desc = "Power Pack Tools" }
    @{ Num = 3; Name = "neovim";            Script = "demo-03-neovim.ps1";            Desc = "Neovim TUI Support" }
    @{ Num = 4; Name = "agent-swarm";       Script = "demo-04-agent-swarm.ps1";       Desc = "Agent Swarm Orchestration" }
    @{ Num = 5; Name = "session-lifecycle"; Script = "demo-05-session-lifecycle.ps1"; Desc = "Session Lifecycle + Resurrection" }
)

$outDir = "$PSScriptRoot"

# Filter to specific demo if requested
if ($Demo -gt 0) {
    $demos = $demos | Where-Object { $_.Num -eq $Demo }
    if (-not $demos) {
        Write-Host "Demo $Demo not found (valid: 1-5)" -ForegroundColor Red
        exit 1
    }
}

foreach ($d in $demos) {
    $castFile = "$outDir/$($d.Name).cast"
    $gifFile  = "$outDir/$($d.Name).gif"
    $mp4File  = "$outDir/$($d.Name).mp4"

    Write-Host "`n=== Demo $($d.Num): $($d.Desc) ===" -ForegroundColor Cyan

    # ── Record ──
    if (-not $GifOnly) {
        Write-Host "  Recording $($d.Script) ..." -ForegroundColor Yellow
        $scriptPath = "$outDir/$($d.Script)"
        if (-not (Test-Path $scriptPath)) {
            Write-Host "  SKIP: $scriptPath not found" -ForegroundColor Red
            continue
        }
        # PowerSession records the terminal session to .cast
        & PowerSession rec -c "pwsh -NoProfile -File `"$scriptPath`"" "$castFile"
        if ($LASTEXITCODE -ne 0) {
            Write-Host "  FAILED: PowerSession recording error" -ForegroundColor Red
            continue
        }
    }

    # ── Convert to GIF ──
    if (Test-Path $castFile) {
        Write-Host "  Rendering GIF ..." -ForegroundColor Green
        & agg "$castFile" "$gifFile" --cols $Cols --rows $Rows --font-size $FontSize
        if (Test-Path $gifFile) {
            $size = (Get-Item $gifFile).Length / 1MB
            Write-Host "  Created: $gifFile ($([math]::Round($size, 1)) MB)" -ForegroundColor Green
        }
    } else {
        Write-Host "  SKIP: $castFile not found (run without -GifOnly first)" -ForegroundColor Yellow
    }

    # ── Convert to MP4 (optional) ──
    if ($Mp4 -and (Test-Path $gifFile)) {
        if (Get-Command ffmpeg -ErrorAction SilentlyContinue) {
            Write-Host "  Rendering MP4 ..." -ForegroundColor Green
            & ffmpeg -y -i "$gifFile" -movflags faststart -pix_fmt yuv420p -vf "scale=trunc(iw/2)*2:trunc(ih/2)*2" "$mp4File" 2>$null
            if (Test-Path $mp4File) {
                $size = (Get-Item $mp4File).Length / 1MB
                Write-Host "  Created: $mp4File ($([math]::Round($size, 1)) MB)" -ForegroundColor Green
            }
        } else {
            Write-Host "  SKIP MP4: ffmpeg not found" -ForegroundColor Yellow
        }
    }
}

Write-Host "`nDone!" -ForegroundColor Green
Write-Host "GIFs are in: $outDir" -ForegroundColor DarkGray
if ($Mp4) { Write-Host "MP4s are in: $outDir" -ForegroundColor DarkGray }
