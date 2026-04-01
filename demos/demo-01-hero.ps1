# Demo 01: Hero — psmux in 30 seconds
# Drives psmux via send-keys, then attaches for PowerSession recording.
#
# Usage:
#   pwsh -NoProfile -File demos/demo-01-hero.ps1           # interactive
#   PowerSession rec -c "pwsh -NoProfile -File demos/demo-01-hero.ps1" demos/hero.cast

. "$PSScriptRoot/lib-demo.ps1"

Demo-Init -SessionName "hero"

# ── Setup: create session with splits ──
psmux new-session -d -s hero
Start-Sleep -Milliseconds 1500

# Type welcome message
Demo-Type "echo 'Welcome to psmux — tmux for Windows'" -Caption "psmux: tmux-compatible multiplexer for Windows"

# Vertical split
Demo-Prefix "%" -Caption "Ctrl+b % — vertical split"

# Type in right pane
Demo-Type "rg --version" -Caption "ripgrep in the right pane"

# Horizontal split in right pane
Demo-Prefix '"' -Caption 'Ctrl+b " — horizontal split'
Demo-Type "fd --version" -Caption "fd in the bottom-right pane"

# Navigate to left pane
Demo-Prefix "Left" -Caption "Ctrl+b Left — navigate panes"
Demo-Type "bat --version" -Caption "bat in the left pane"

# Zoom the left pane
Demo-Prefix "z" -Caption "Ctrl+b z — zoom pane (fullscreen)"
Demo-Wait 2000
Demo-Prefix "z" -Caption "Ctrl+b z — unzoom"

# Rename window
Demo-Prefix "," -Caption "Ctrl+b , — rename window"
Start-Sleep -Milliseconds 500
psmux send-keys -t hero "dev" Enter
Demo-Caption "Renamed to 'dev'"
Demo-Wait 1000

# New window
Demo-Prefix "c" -Caption "Ctrl+b c — new window"
Demo-Type "echo 'Window 2 — monitoring'" -Caption "Second window for monitoring"

# Switch back
Demo-Send "0" -Caption "Ctrl+b 0 — switch to window 0"
Demo-Wait 1500

Demo-Type "echo '# 92 tmux commands, native Windows, zero WSL'" -Caption "92 tmux commands. Native Windows. No WSL required."

# Save captions
Demo-SaveCaptions "$PSScriptRoot/hero.srt"

# ── Attach for visual recording ──
Write-Host "Attaching to session (press Ctrl+b d to detach when done)..." -ForegroundColor Cyan
Demo-Wait 500
Demo-Attach
