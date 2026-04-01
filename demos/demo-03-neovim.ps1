# Demo 03: Neovim in psmux — Full TUI Support
# Neovim interactions use send-keys (keystrokes go to nvim).
# Pane management uses direct CLI commands.

. "$PSScriptRoot/lib-demo.ps1"

Demo-Init -SessionName "nvim"

Demo-Run "new-session -d -s nvim" -WaitMs 1500

# ── Enable title bars ──
Demo-Run "set -g pane-border-status top" -Caption "Per-pane title bars" -WaitMs 800

# ── Open neovim ──
Demo-ShellCmd "nvim src/hints.rs" -Caption "Open neovim — cursor is block (normal mode)" -WaitMs 2500

# ── Navigate in nvim (these are real keystrokes to nvim) ──
& psmux send-keys -t nvim j j j j j j j j j j
Demo-Caption "Navigate with j/k — cursor stays block shape"
Demo-Wait 1500

# ── Search ──
& psmux send-keys -t nvim / f n Space s c a n Enter
Demo-Caption "Search with / — full vi keybindings work"
Demo-Wait 2000

# ── Insert mode — cursor changes to bar ──
& psmux send-keys -t nvim i
Demo-Caption "Insert mode — cursor changes to bar (DECSCUSR)"
Demo-Wait 1500

& psmux send-keys -t nvim -l -- "// psmux tracks cursor style"
Demo-Wait 500
& psmux send-keys -t nvim Escape
Demo-Caption "Escape — cursor restores to block"
Demo-Wait 1500

# ── Undo ──
& psmux send-keys -t nvim u
Demo-Caption "Undo with u"
Demo-Wait 1000

# ── Split pane (direct command) ──
Demo-Run "split-window -h -t nvim" -Caption "Ctrl+b % — split while neovim runs" -WaitMs 1200

# ── Open another file ──
Demo-ShellCmd "nvim src/rendering.rs" -Caption "Second neovim in right pane" -WaitMs 2500

# ── Switch panes — focus events fire ──
Demo-Run "select-pane -L -t nvim" `
    -Caption "Switch panes — FocusOut/FocusIn events fire" -WaitMs 2000

Demo-Run "select-pane -R -t nvim" `
    -Caption "Back — cursor style restored automatically" -WaitMs 2000

# ── Visual selection ──
& psmux send-keys -t nvim V
Demo-Caption "Visual line mode"
Demo-Wait 500
& psmux send-keys -t nvim j j j
Demo-Wait 800
& psmux send-keys -t nvim y
Demo-Caption "Yank selection"
Demo-Wait 1000

# ── Quit both ──
& psmux send-keys -t nvim : q ! Enter
Demo-Wait 500
Demo-Run "select-pane -L -t nvim" -WaitMs 300
& psmux send-keys -t nvim : q ! Enter
Demo-Wait 500

Demo-ShellCmd "echo Mouse clicks and scroll also work" `
    -Caption "Mouse, cursor shapes, focus events, bracketed paste — all work"

Demo-SaveCaptions "$PSScriptRoot/neovim.srt"
Demo-Attach -Target "nvim"
