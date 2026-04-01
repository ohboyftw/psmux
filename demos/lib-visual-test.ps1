# Visual test framework for psmux demos
# Captures screenshots of Windows Terminal at each step,
# optionally validates with a VLM (Claude, GPT-4o, etc.)
#
# Usage: source this alongside lib-demo.ps1
#   . "$PSScriptRoot/lib-demo.ps1"
#   . "$PSScriptRoot/lib-visual-test.ps1"

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$script:ScreenshotDir = ""
$script:ScreenshotIndex = 0
$script:Assertions = @()
$script:WindowTitle = "Windows Terminal"

function VTest-Init {
    param(
        [string]$OutputDir = "$PSScriptRoot/screenshots",
        [string]$WindowTitle = "Windows Terminal"
    )
    $script:ScreenshotDir = $OutputDir
    $script:ScreenshotIndex = 0
    $script:Assertions = @()
    $script:WindowTitle = $WindowTitle

    if (-not (Test-Path $OutputDir)) {
        New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
    }
}

function VTest-CaptureWindow {
    # Capture a specific window by title using Win32 API
    param([string]$Title = "")
    $t = if ($Title) { $Title } else { $script:WindowTitle }

    Add-Type @"
    using System;
    using System.Runtime.InteropServices;
    using System.Drawing;
    using System.Drawing.Imaging;
    public class WindowCapture {
        [DllImport("user32.dll")] static extern IntPtr FindWindow(string lpClassName, string lpWindowName);
        [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
        [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr hWnd);
        [StructLayout(LayoutKind.Sequential)] public struct RECT {
            public int Left, Top, Right, Bottom;
        }
        public static Bitmap Capture(string title) {
            IntPtr hwnd = FindWindow(null, title);
            if (hwnd == IntPtr.Zero) return null;
            SetForegroundWindow(hwnd);
            System.Threading.Thread.Sleep(200);
            RECT r;
            GetWindowRect(hwnd, out r);
            int w = r.Right - r.Left;
            int h = r.Bottom - r.Top;
            if (w <= 0 || h <= 0) return null;
            var bmp = new Bitmap(w, h, PixelFormat.Format32bppArgb);
            using (var g = Graphics.FromImage(bmp)) {
                g.CopyFromScreen(r.Left, r.Top, 0, 0, new Size(w, h));
            }
            return bmp;
        }
    }
"@ -ErrorAction SilentlyContinue

    try {
        $bmp = [WindowCapture]::Capture($t)
        if ($null -eq $bmp) {
            # Fallback: try partial title match via process
            $proc = Get-Process | Where-Object { $_.MainWindowTitle -like "*$t*" } | Select-Object -First 1
            if ($proc) {
                $bmp = [WindowCapture]::Capture($proc.MainWindowTitle)
            }
        }
        return $bmp
    } catch {
        Write-Host "    Screenshot failed: $_" -ForegroundColor Yellow
        return $null
    }
}

function VTest-Screenshot {
    # Take a screenshot and save it with an incremental name
    param(
        [string]$Label = "",
        [string]$Assertion = ""
    )
    $script:ScreenshotIndex++
    $idx = $script:ScreenshotIndex.ToString("D3")
    $safeName = if ($Label) { $Label -replace '[^a-zA-Z0-9_-]', '_' } else { "step" }
    $filename = "${idx}_${safeName}.png"
    $filepath = Join-Path $script:ScreenshotDir $filename

    $bmp = VTest-CaptureWindow
    if ($bmp) {
        $bmp.Save($filepath, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        Write-Host "    Screenshot: $filename" -ForegroundColor DarkGray
    } else {
        Write-Host "    Screenshot FAILED: $filename" -ForegroundColor Yellow
    }

    if ($Assertion) {
        $script:Assertions += [PSCustomObject]@{
            Index      = $script:ScreenshotIndex
            File       = $filename
            Assertion  = $Assertion
            Result     = "pending"  # filled in by VLM validation pass
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
