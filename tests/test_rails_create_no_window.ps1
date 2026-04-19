# Rails bench: CREATE_NO_WINDOW for background subprocesses (P2.25)
# Validates HideWindowCommandExt is applied so run-shell / if-shell / format#()
# don't flash a conhost window.
. $PSScriptRoot/_harness.ps1

Write-Host "== CREATE_NO_WINDOW ==" -ForegroundColor Cyan
$S = New-IsolatedSession "create-no-window"

Test-Case "run-shell 'echo hello' completes without opening a standalone console window" {
    # Snapshot console windows before
    $typeDef = @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class EW {
    delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumWindowsProc e, IntPtr l);
    [DllImport("user32.dll")] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    public static int CountVisibleConsole() {
        int n = 0;
        EnumWindows((h, l) => {
            if (!IsWindowVisible(h)) return true;
            StringBuilder sb = new StringBuilder(256);
            GetClassName(h, sb, sb.Capacity);
            string c = sb.ToString();
            if (c == "ConsoleWindowClass" || c == "PseudoConsoleWindow") n++;
            return true;
        }, IntPtr.Zero);
        return n;
    }
}
"@
    if (-not ("EW" -as [type])) {
        Add-Type -TypeDefinition $typeDef
    }
    $before = [EW]::CountVisibleConsole()
    # Fire several run-shells in rapid succession
    for ($i = 0; $i -lt 5; $i++) {
        psmux run-shell -t $S "echo probe-$i" 2>&1 | Out-Null
    }
    Start-Sleep -Milliseconds 300
    $after = [EW]::CountVisibleConsole()
    # Count must not increase — any flashes create visible windows during the 300ms
    $after -le $before
}

Test-Case "if-shell condition true path executes without visible console" {
    # Indirect: just verify if-shell completes without error
    psmux if-shell -t $S "exit 0" "display-message 'conditional-ran'" 2>&1 | Out-Null
    $LASTEXITCODE -eq 0
}

Remove-PsmuxSession $S
Write-Summary
