# Demo 01: Hero — psmux in 30 seconds
#
# Drives psmux via CLI commands. At each step, captures a screenshot
# and registers a visual assertion for VLM validation.
#
# Usage:
#   pwsh -NoProfile -File demos/demo-01-hero.ps1                    # run demo
#   pwsh -NoProfile -File demos/demo-01-hero.ps1 -Validate          # run + VLM check
#
# Output:
#   demos/screenshots/hero/001_*.png ... N screenshots
#   demos/screenshots/hero/manifest.json  assertion manifest
#   demos/screenshots/hero/demo.gif       stitched GIF
#   demos/hero.srt                        subtitle captions

param([switch]$Validate)

. "$PSScriptRoot/lib-demo.ps1"
. "$PSScriptRoot/lib-visual-test.ps1"

Demo-Init -SessionName "hero"
VTest-Init -OutputDir "$PSScriptRoot/screenshots/hero" -WindowTitle "psmux-demo"

# ── Step 1: Create a named session (attached) ──
# Start psmux in a standalone conhost window (bypasses Windows Terminal
# tab absorption). The "title" command gives it a fixed, findable name
# for screenshot capture. conhost.exe guarantees a separate window.
$psmuxProc = Start-Process -FilePath "conhost.exe" `
    -ArgumentList "cmd.exe /c `"title psmux-demo && psmux new-session -s hero`"" `
    -PassThru -WindowStyle Normal
Start-Sleep -Milliseconds 5000  # warm pane + PowerShell profile load

VTest-Assert -Label "session_created" `
    -Should "Terminal shows a psmux session with a green status bar at the bottom showing [hero]"

# ── Step 2: Type welcome message ──
& psmux send-keys -t hero -l -- "echo Welcome to psmux"
& psmux send-keys -t hero Enter
Start-Sleep -Milliseconds 1500
Demo-Caption "psmux: tmux-compatible multiplexer for Windows"

VTest-Assert -Label "welcome_typed" `
    -Should "The pane shows 'Welcome to psmux' text output"

# ── Step 3: Vertical split ──
& psmux split-window -h -t hero
Start-Sleep -Milliseconds 1500
Demo-Caption "Ctrl+b % — vertical split"

VTest-Assert -Label "vertical_split" `
    -Should "Two panes side by side separated by a vertical border line"

# ── Step 4: Type in right pane ──
& psmux send-keys -t hero -l -- "rg --version"
& psmux send-keys -t hero Enter
Start-Sleep -Milliseconds 1200
Demo-Caption "ripgrep in the right pane"

VTest-Assert -Label "right_pane_rg" `
    -Should "Right pane shows ripgrep version output"

# ── Step 5: Horizontal split ──
& psmux split-window -v -t hero
Start-Sleep -Milliseconds 1500
Demo-Caption 'Ctrl+b " — horizontal split'

VTest-Assert -Label "three_panes" `
    -Should "Three panes visible: one large on left, two stacked on right"

# ── Step 6: Type in bottom-right ──
& psmux send-keys -t hero -l -- "fd --version"
& psmux send-keys -t hero Enter
Start-Sleep -Milliseconds 1200
Demo-Caption "fd in the bottom-right pane"

# ── Step 7: Navigate to left pane ──
& psmux select-pane -L -t hero
Start-Sleep -Milliseconds 800
Demo-Caption "Ctrl+b Left — navigate panes"

& psmux send-keys -t hero -l -- "bat --version"
& psmux send-keys -t hero Enter
Start-Sleep -Milliseconds 1200
Demo-Caption "bat in the left pane"

VTest-Assert -Label "three_panes_with_tools" `
    -Should "Three panes each showing a different tool version output (bat, rg, fd)"

# ── Step 8: Zoom pane ──
& psmux resize-pane -Z -t hero
Start-Sleep -Milliseconds 2000
Demo-Caption "Ctrl+b z — zoom pane (fullscreen)"

VTest-Assert -Label "zoomed" `
    -Should "Single pane taking the full terminal area, no split borders visible"

& psmux resize-pane -Z -t hero
Start-Sleep -Milliseconds 1500
Demo-Caption "Ctrl+b z — unzoom"

VTest-Assert -Label "unzoomed" `
    -Should "Three panes restored with split borders"

# ── Step 9: Rename window ──
& psmux rename-window -t hero dev
Start-Sleep -Milliseconds 1200
Demo-Caption "Ctrl+b , — rename window to 'dev'"

VTest-Assert -Label "renamed" `
    -Should "Status bar shows window name 'dev' instead of default"

# ── Step 10: New window + switch ──
& psmux new-window -t hero
Start-Sleep -Milliseconds 1500
Demo-Caption "Ctrl+b c — new window"

& psmux send-keys -t hero -l -- "echo Window 2 - monitoring"
& psmux send-keys -t hero Enter
Start-Sleep -Milliseconds 1000

& psmux select-window -t hero:0
Start-Sleep -Milliseconds 1500
Demo-Caption "Ctrl+b 0 — switch to window 0"

VTest-Assert -Label "back_to_window0" `
    -Should "Three-pane layout from window 0 visible again, status bar shows window 0 highlighted"

# ── Step 11: Final message ──
& psmux send-keys -t hero -l -- "echo 92 tmux commands. Native Windows. No WSL."
& psmux send-keys -t hero Enter
Start-Sleep -Milliseconds 2000
Demo-Caption "92 tmux commands. Native Windows. No WSL required."

VTest-Assert -Label "final" `
    -Should "Pane shows '92 tmux commands' message, three-pane layout intact"

# ── Save outputs ──
Demo-SaveCaptions "$PSScriptRoot/hero.srt"
VTest-SaveManifest
VTest-MakeGif -OutputPath "$PSScriptRoot/hero.gif"

# ── Cleanup ──
Write-Host "`nDemo complete. Session 'hero' still running." -ForegroundColor Green
Write-Host "  Attach:  psmux attach -t hero" -ForegroundColor DarkGray
Write-Host "  Cleanup: psmux kill-session -t hero" -ForegroundColor DarkGray
