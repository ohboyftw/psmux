# Record all psmux demos using PowerSession + agg
#
# Usage:
#   pwsh -NoProfile -File demos/record-all.ps1              # record all, GIF output
#   pwsh -NoProfile -File demos/record-all.ps1 -Demo 2      # record demo 02 only
#   pwsh -NoProfile -File demos/record-all.ps1 -GifOnly      # re-render GIFs from existing .cast
#   pwsh -NoProfile -File demos/record-all.ps1 -Mp4           # also render MP4 with subtitles
#
# Pipeline:
#   1. Demo script creates psmux session, drives it via send-keys
#   2. Demo script generates .srt caption file
#   3. Demo script attaches — PowerSession records the visual session
#   4. agg converts .cast → .gif
#   5. ffmpeg converts .cast → .mp4 with burned-in subtitles (optional)

param(
    [int]$Demo = 0,
    [switch]$GifOnly,
    [switch]$Mp4,
    [int]$Cols = 120,
    [int]$Rows = 35,
    [int]$FontSize = 16
)

$ErrorActionPreference = 'Stop'

$demos = @(
    @{ Num = 1; Name = "hero";              Script = "demo-01-hero.ps1";              Srt = "hero.srt" }
    @{ Num = 2; Name = "powerpack-tools";   Script = "demo-02-powerpack-tools.ps1";   Srt = "powerpack.srt" }
    @{ Num = 3; Name = "neovim";            Script = "demo-03-neovim.ps1";            Srt = "neovim.srt" }
    @{ Num = 4; Name = "agent-swarm";       Script = "demo-04-agent-swarm.ps1";       Srt = "agent-swarm.srt" }
    @{ Num = 5; Name = "session-lifecycle"; Script = "demo-05-session-lifecycle.ps1"; Srt = "session-lifecycle.srt" }
)

$outDir = $PSScriptRoot

if ($Demo -gt 0) {
    $demos = $demos | Where-Object { $_.Num -eq $Demo }
    if (-not $demos) { Write-Host "Demo $Demo not found (1-5)" -ForegroundColor Red; exit 1 }
}

foreach ($d in $demos) {
    $castFile = "$outDir/$($d.Name).cast"
    $gifFile  = "$outDir/$($d.Name).gif"
    $mp4File  = "$outDir/$($d.Name).mp4"
    $srtFile  = "$outDir/$($d.Srt)"

    Write-Host "`n=== Demo $($d.Num): $($d.Name) ===" -ForegroundColor Cyan

    if (-not $GifOnly) {
        $scriptPath = "$outDir/$($d.Script)"
        if (-not (Test-Path $scriptPath)) {
            Write-Host "  SKIP: $scriptPath not found" -ForegroundColor Red
            continue
        }
        Write-Host "  Recording (Ctrl+b d to stop) ..." -ForegroundColor Yellow
        & PowerSession rec -c "pwsh -NoProfile -File `"$scriptPath`"" "$castFile"
    }

    # GIF
    if (Test-Path $castFile) {
        Write-Host "  Rendering GIF ..." -ForegroundColor Green
        & agg "$castFile" "$gifFile" --cols $Cols --rows $Rows --font-size $FontSize
        if (Test-Path $gifFile) {
            $sz = [math]::Round((Get-Item $gifFile).Length / 1MB, 1)
            Write-Host "  $gifFile ($sz MB)" -ForegroundColor Green
        }
    }

    # MP4 with subtitles
    if ($Mp4 -and (Test-Path $gifFile)) {
        if (Get-Command ffmpeg -ErrorAction SilentlyContinue) {
            Write-Host "  Rendering MP4 ..." -ForegroundColor Green
            $subFilter = ""
            if (Test-Path $srtFile) {
                $escaped = $srtFile.Replace('\', '/').Replace(':', '\:')
                $subFilter = ",subtitles='$escaped':force_style='FontSize=14,PrimaryColour=&H00FFFFFF,OutlineColour=&H00000000,Outline=2,MarginV=30'"
            }
            & ffmpeg -y -i "$gifFile" `
                -movflags faststart -pix_fmt yuv420p `
                -vf "scale=trunc(iw/2)*2:trunc(ih/2)*2$subFilter" `
                "$mp4File" 2>$null
            if (Test-Path $mp4File) {
                $sz = [math]::Round((Get-Item $mp4File).Length / 1MB, 1)
                Write-Host "  $mp4File ($sz MB)" -ForegroundColor Green
            }
        } else {
            Write-Host "  SKIP MP4: ffmpeg not found" -ForegroundColor Yellow
        }
    }
}

Write-Host "`nDone! Outputs in: $outDir" -ForegroundColor Green
