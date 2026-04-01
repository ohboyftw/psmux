# Demo 03: Neovim in psmux — Full TUI Support
# Shows: cursor shape tracking, focus events, pane titles, split navigation
#
# Note: Mouse clicks can't be scripted via send-keys — mention in captions
# that mouse works, show keyboard-driven features that prove TUI support.

. "$PSScriptRoot/lib-demo.ps1"

Demo-Init -SessionName "nvim"

psmux new-session -d -s nvim
Start-Sleep -Milliseconds 1500

# Enable pane title bars
Demo-Type "psmux set -g pane-border-status top" -Caption "Enable per-pane title bars"

# Open neovim
Demo-Type "nvim src/hints.rs" -Caption "Open neovim — cursor is block (normal mode)" -WaitMs 2500

# Navigate
Demo-Send "j j j j j j j j j j" -Caption "Navigate with j/k — cursor stays block shape" -WaitMs 1500

# Search
psmux send-keys -t nvim "/fn scan" Enter
Demo-Caption "Search with / — full vi keybindings work"
Demo-Wait 2000

# Enter insert mode — cursor changes to bar
Demo-Send "i" -Caption "Insert mode — cursor changes to bar (DECSCUSR)" -WaitMs 1500
psmux send-keys -t nvim "// psmux tracks cursor style!" Escape
Demo-Caption "Type text, then Escape back to normal — cursor restores to block"
Demo-Wait 1500

# Undo
Demo-Send "u" -Caption "Undo with u" -WaitMs 1000

# Split pane
Demo-Prefix "%" -Caption "Ctrl+b % — split while neovim runs"

# Open another file
Demo-Type "nvim src/rendering.rs" -Caption "Second neovim instance in right pane" -WaitMs 2500

# Switch panes — focus events fire
Demo-Prefix "Left" -Caption "Switch panes — FocusOut/FocusIn events fire (:checktime triggers)"
Demo-Wait 2000

Demo-Prefix "Right" -Caption "Back to right pane — cursor style restored automatically"
Demo-Wait 2000

# Visual selection
Demo-Send "V" -Caption "Visual line mode — selection highlighting works"
Demo-Wait 500
Demo-Send "j j j" -WaitMs 800
Demo-Send "y" -Caption "Yank selection"
Demo-Wait 1000

# Quit both
psmux send-keys -t nvim ":q!" Enter
Demo-Wait 500
Demo-Prefix "Left"
psmux send-keys -t nvim ":q!" Enter
Demo-Wait 500

Demo-Type "echo '# Mouse clicks, scroll, drag also work — try it!'" `
    -Caption "Mouse, cursor shapes, focus events, bracketed paste — all work"

Demo-SaveCaptions "$PSScriptRoot/neovim.srt"

Write-Host "Attaching..." -ForegroundColor Cyan
Demo-Wait 500
Demo-Attach
