# Demo 02: Power Pack Tools — The Linux CLI experience on Windows
# All commands via send-keys -l (literal) or direct psmux commands.

. "$PSScriptRoot/lib-demo.ps1"

Demo-Init -SessionName "tools"

Demo-Run "new-session -d -s tools" -WaitMs 1500

# ── fastfetch ──
Demo-ShellCmd "fastfetch" -Caption "fastfetch — system info at a glance" -WaitMs 3000

# ── ripgrep ──
Demo-ShellCmd "rg --version" -Caption "ripgrep — 10-100x faster than grep" -WaitMs 1000
Demo-ShellCmd "rg fn write_mouse --type rust -C2" `
    -Caption "rg: search code with context, respects .gitignore" -WaitMs 2500

# ── fd ──
Demo-ShellCmd "fd --extension rs src/ --max-depth 1" `
    -Caption "fd — smart find replacement" -WaitMs 2000

# ── bat ──
Demo-ShellCmd "bat src/hints.rs --range 1:25" `
    -Caption "bat — cat with syntax highlighting + line numbers" -WaitMs 3000

# ── Split for side-by-side ──
Demo-Run "split-window -h -t tools" -Caption "Split pane for side-by-side workflow" -WaitMs 1000

# ── television ──
Demo-ShellCmd "tv files" -Caption "television — interactive fuzzy file picker with preview" -WaitMs 3000
# Exit tv
& psmux send-keys -t tools Escape
Demo-Wait 1000

# ── Navigate left, show zoxide ──
Demo-Run "select-pane -L -t tools" -Caption "Switch to left pane" -WaitMs 800
Demo-ShellCmd "z psmux" -Caption "zoxide — smart cd: 'z psmux' jumps instantly" -WaitMs 1200
Demo-ShellCmd "pwd" -WaitMs 1000

# ── Combined pipeline ──
Demo-ShellCmd "fd config --type f | head -3" `
    -Caption "Combine tools: fd finds, bat previews" -WaitMs 2500

# ── fzf ──
Demo-ShellCmd "echo fzf demo" -Caption "fzf — fuzzy match anything piped to it" -WaitMs 1500

# ── Final ──
Demo-ShellCmd "echo install.ps1 -Full sets up all tools" `
    -Caption "One command installs everything: install.ps1 -Full"

Demo-SaveCaptions "$PSScriptRoot/powerpack.srt"
Demo-Attach -Target "tools"
