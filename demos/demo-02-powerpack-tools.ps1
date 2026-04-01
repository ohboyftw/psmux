# Demo 02: Power Pack Tools — The Linux CLI experience on Windows
# Shows: ripgrep, fd, bat, television, zoxide, fzf, fastfetch, starship
#
# Usage:
#   pwsh -NoProfile -File demos/demo-02-powerpack-tools.ps1
#   PowerSession rec -c "pwsh -NoProfile -File demos/demo-02-powerpack-tools.ps1" demos/powerpack.cast

. "$PSScriptRoot/lib-demo.ps1"

Demo-Init -SessionName "tools"

psmux new-session -d -s tools
Start-Sleep -Milliseconds 1500

# ── fastfetch: system info splash ──
Demo-Type "fastfetch" -Caption "fastfetch — system info at a glance" -WaitMs 3000

# ── ripgrep: fast code search ──
Demo-Type "rg 'fn write_mouse' --type rust -C2" -Caption "ripgrep — search code 10-100x faster than grep" -WaitMs 2500
Demo-Type "rg 'focus_reporting' --type rust" -Caption "ripgrep — find every reference instantly" -WaitMs 2000

# ── fd: fast file finder ──
Demo-Type "fd '\.rs$' src/ --max-depth 1" -Caption "fd — smart find replacement, respects .gitignore" -WaitMs 2000
Demo-Type "fd test --type f --extension rs" -Caption "fd — find all test files" -WaitMs 2000

# ── bat: syntax-highlighted viewing ──
Demo-Type "bat src/hints.rs --range 1:25" -Caption "bat — cat with syntax highlighting + line numbers" -WaitMs 3000

# ── Split for side-by-side tools demo ──
Demo-Prefix "%" -Caption "Split pane for side-by-side workflow"

# ── television: fuzzy TUI picker ──
Demo-Type "tv files" -Caption "television — interactive fuzzy file picker with preview" -WaitMs 3000
psmux send-keys -t tools Escape  # exit tv
Demo-Wait 1000

# ── Navigate left, show zoxide ──
Demo-Prefix "Left" -Caption "Switch to left pane"
Demo-Type "z psmux" -Caption "zoxide — smart cd: 'z psmux' jumps to D:\\Home\\psmux" -WaitMs 1500
Demo-Type "pwd" -WaitMs 1000

# ── Combined pipeline ──
Demo-Type "fd config --type f | head -5 | xargs bat --style=header,grid" `
    -Caption "Combine tools: fd | bat — find configs and preview them" -WaitMs 3500

# ── fzf with bat preview ──
Demo-Type "rg --files | fzf --preview 'bat --color=always {}'" `
    -Caption "fzf + bat — fuzzy file finder with syntax preview" -WaitMs 3000
psmux send-keys -t tools Escape  # exit fzf
Demo-Wait 1000

Demo-Type "echo '# install.ps1 -Full sets up all tools automatically'" `
    -Caption "One command installs everything: install.ps1 -Full"

Demo-SaveCaptions "$PSScriptRoot/powerpack.srt"

Write-Host "Attaching..." -ForegroundColor Cyan
Demo-Wait 500
Demo-Attach
