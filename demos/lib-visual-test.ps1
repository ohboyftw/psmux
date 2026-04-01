# Visual test framework for psmux demos
# Captures screenshots of Windows Terminal at each step,
# optionally validates with a VLM (Claude, GPT-4o, etc.)
#
# Usage: source this alongside lib-demo.ps1
#   . "$PSScriptRoot/lib-demo.ps1"
#   . "$PSScriptRoot/lib-visual-test.ps1"

$script:ScreenshotDir = ""
$script:ScreenshotIndex = 0
$script:Assertions = @()
$script:WindowTitle = "Windows Terminal"
$script:CaptureReady = $false

function VTest-Init {
    param(
        [string]$OutputDir = "$PSScriptRoot/screenshots",
        [string]$WindowTitle = "Windows Terminal"
    )
    $script:ScreenshotDir = $OutputDir
    $script:ScreenshotIndex = 0
    $script:Assertions = @()
    $script:WindowTitle = $WindowTitle
    $script:CaptureReady = $false

    if (-not (Test-Path $OutputDir)) {
        New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
    }

    # Compile the Win32 capture helper once using Windows PowerShell
    # (pwsh 7 on .NET Core lacks System.Drawing; Windows PS 5.1 has it)
    $helperScript = Join-Path $OutputDir "_capture_helper.ps1"
    @'
param([string]$Title, [string]$OutPath)
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinApi {
    [DllImport("user32.dll")] public static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWinProc cb, IntPtr lp);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd, System.Text.StringBuilder sb, int max);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    public delegate bool EnumWinProc(IntPtr hwnd, IntPtr lp);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
# Find window — exact match first, then partial
$hwnd = [WinApi]::FindWindow([NullString]::Value, $Title)
if ($hwnd -eq [IntPtr]::Zero) {
    # Partial match via EnumWindows
    $found = [IntPtr]::Zero
    $cb = [WinApi+EnumWinProc]{
        param($h, $l)
        if ([WinApi]::IsWindowVisible($h)) {
            $sb = New-Object System.Text.StringBuilder 512
            [void][WinApi]::GetWindowText($h, $sb, 512)
            if ($sb.ToString() -like "*$Title*") {
                $script:found = $h
                return $false
            }
        }
        return $true
    }
    [WinApi]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
    $hwnd = $found
}
if ($hwnd -eq [IntPtr]::Zero) { Write-Error "Window not found: $Title"; exit 1 }
[WinApi]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 300
$r = New-Object WinApi+RECT
[WinApi]::GetWindowRect($hwnd, [ref]$r) | Out-Null
$w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
if ($w -le 0 -or $h -le 0) { Write-Error "Invalid window rect"; exit 1 }
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
$g.Dispose()
$bmp.Save($OutPath, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
'@ | Set-Content -Path $helperScript -Encoding UTF8
    $script:CaptureReady = $true
}

function VTest-CaptureWindow {
    param([string]$OutPath)
    if (-not $script:CaptureReady) { return $false }
    $helper = Join-Path $script:ScreenshotDir "_capture_helper.ps1"
    try {
        # Run in Windows PowerShell 5.1 which has System.Drawing
        $result = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $helper -Title $script:WindowTitle -OutPath $OutPath 2>&1
        if (Test-Path $OutPath) { return $true }
        Write-Host "    Capture output: $result" -ForegroundColor Yellow
        return $false
    } catch {
        Write-Host "    Screenshot failed: $_" -ForegroundColor Yellow
        return $false
    }
}

function VTest-Screenshot {
    param(
        [string]$Label = "",
        [string]$Assertion = ""
    )
    $script:ScreenshotIndex++
    $idx = $script:ScreenshotIndex.ToString("D3")
    $safeName = if ($Label) { $Label -replace '[^a-zA-Z0-9_-]', '_' } else { "step" }
    $filename = "${idx}_${safeName}.png"
    $filepath = Join-Path $script:ScreenshotDir $filename

    $ok = VTest-CaptureWindow -OutPath $filepath
    if ($ok) {
        Write-Host "    Screenshot: $filename" -ForegroundColor DarkGray
    } else {
        Write-Host "    Screenshot FAILED: $filename" -ForegroundColor Yellow
    }

    if ($Assertion) {
        $script:Assertions += [PSCustomObject]@{
            Index      = $script:ScreenshotIndex
            File       = $filename
            Assertion  = $Assertion
            Result     = "pending"
        }
    }

    return $filepath
}

function VTest-Assert {
    # Take a screenshot and register a visual assertion
    param(
        [string]$Label,
        [string]$Should
    )
    VTest-Screenshot -Label $Label -Assertion $Should
}

function VTest-SaveManifest {
    # Write assertion manifest for VLM validation pass
    param([string]$Path = "")
    if (-not $Path) { $Path = Join-Path $script:ScreenshotDir "manifest.json" }

    $manifest = @{
        total       = $script:Assertions.Count
        screenshots = $script:ScreenshotDir
        assertions  = $script:Assertions | ForEach-Object {
            @{
                index     = $_.Index
                file      = $_.File
                assertion = $_.Assertion
                result    = $_.Result
            }
        }
    } | ConvertTo-Json -Depth 3

    Set-Content -Path $Path -Value $manifest -Encoding UTF8
    Write-Host "`n  Manifest: $Path ($($script:Assertions.Count) assertions)" -ForegroundColor Cyan
}

function VTest-MakeGif {
    # Stitch screenshots into a GIF using ffmpeg
    param(
        [string]$OutputPath = "",
        [double]$FrameRate = 0.5  # 0.5 fps = 2 seconds per frame
    )
    if (-not $OutputPath) { $OutputPath = Join-Path $script:ScreenshotDir "demo.gif" }

    if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) {
        Write-Host "  ffmpeg not found — skipping GIF generation" -ForegroundColor Yellow
        return
    }

    $pattern = Join-Path $script:ScreenshotDir "%03d_*.png"
    # Use glob input for ffmpeg
    $pngs = Get-ChildItem $script:ScreenshotDir -Filter "*.png" | Sort-Object Name
    if ($pngs.Count -eq 0) {
        Write-Host "  No screenshots to stitch" -ForegroundColor Yellow
        return
    }

    # Create a concat file for ffmpeg
    $concatFile = Join-Path $script:ScreenshotDir "concat.txt"
    $lines = $pngs | ForEach-Object {
        "file '$($_.FullName)'`nduration 2"
    }
    Set-Content -Path $concatFile -Value ($lines -join "`n") -Encoding UTF8

    & ffmpeg -y -f concat -safe 0 -i $concatFile `
        -vf "scale=1280:-1:flags=lanczos,fps=1" `
        -loop 0 $OutputPath 2>$null

    Remove-Item $concatFile -ErrorAction SilentlyContinue

    if (Test-Path $OutputPath) {
        $sz = [math]::Round((Get-Item $OutputPath).Length / 1MB, 1)
        Write-Host "  GIF: $OutputPath ($sz MB)" -ForegroundColor Green
    }
}
