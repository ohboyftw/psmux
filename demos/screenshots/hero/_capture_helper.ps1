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
