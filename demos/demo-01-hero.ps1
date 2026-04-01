# Demo 01: Hero — psmux in 30 seconds
# All pane management uses direct CLI commands (not send-keys for prefix actions).
# Shell commands use send-keys -l (literal mode) to avoid quoting issues.

. "$PSScriptRoot/lib-demo.ps1"

Demo-Init -SessionName "hero"

# ── Create session ──
Demo-Run "new-session -d -s hero" -Caption "psmux new-session -s hero" -WaitMs 1500

# ── Type welcome ──
Demo-ShellCmd "echo Welcome to psmux" -Caption "psmux: tmux-compatible multiplexer for Windows"

# ── Vertical split (direct command, not Ctrl+b %) ──
Demo-Run "split-window -h -t hero" -Caption "Ctrl+b % — vertical split" -WaitMs 1200

# ── Type in right pane ──
Demo-ShellCmd "rg --version" -Caption "ripgrep in the right pane"

# ── Horizontal split ──
Demo-Run "split-window -v -t hero" -Caption 'Ctrl+b " — horizontal split' -WaitMs 1200

Demo-ShellCmd "fd --version" -Caption "fd in the bottom-right pane"

# ── Navigate to left pane ──
Demo-Run "select-pane -L -t hero" -Caption "Ctrl+b Left — navigate panes" -WaitMs 800

Demo-ShellCmd "bat --version" -Caption "bat in the left pane"

# ── Zoom pane ──
Demo-Run "resize-pane -Z -t hero" -Caption "Ctrl+b z — zoom pane (fullscreen)" -WaitMs 2000
Demo-Run "resize-pane -Z -t hero" -Caption "Ctrl+b z — unzoom" -WaitMs 1500

# ── Rename window ──
Demo-Run "rename-window -t hero dev" -Caption "Ctrl+b , — rename window to 'dev'" -WaitMs 1200

# ── New window ──
Demo-Run "new-window -t hero" -Caption "Ctrl+b c — new window" -WaitMs 1200
Demo-ShellCmd "echo Window 2 - monitoring" -Caption "Second window for monitoring"

# ── Switch back to window 0 ──
Demo-Run "select-window -t hero:0" -Caption "Ctrl+b 0 — switch to window 0" -WaitMs 1500

# ── Final message ──
Demo-ShellCmd "echo 92 tmux commands. Native Windows. No WSL." `
    -Caption "92 tmux commands. Native Windows. No WSL required."

Demo-Wait 2000
Demo-SaveCaptions "$PSScriptRoot/hero.srt"
Demo-Attach -Target "hero"
