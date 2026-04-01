# Demo 02: Power Pack Tools — The Linux CLI experience on Windows
# Shows: ripgrep, fd, bat, television, zoxide, fzf, starship, fastfetch
#
# Short (README): 20s — rg + fd + bat + tv quick hits
# Full (social): 45s — all tools with real project search workflow

$delay = 50

function Type($text) { foreach ($c in $text.ToCharArray()) { [Console]::Write($c); Start-Sleep -Milliseconds $delay } }
function Enter { [Console]::Write("`r`n"); Start-Sleep -Milliseconds 300 }
function Wait($ms) { Start-Sleep -Milliseconds $ms }
function Pause { Start-Sleep -Milliseconds 1500 }

# ── Scene 1: fastfetch — system at a glance ──
Wait 500
Type "fastfetch"
Enter
Wait 3000

# ── Scene 2: ripgrep — search code instantly ──
Type "# ripgrep: 10-100x faster than grep, respects .gitignore"
Enter
Wait 800
Type "rg 'fn write_mouse' --type rust -C2"
Enter
Wait 2000

Type "rg 'focus_reporting' --type rust"
Enter
Wait 1500

# ── Scene 3: fd — find files fast ──
Type "# fd: smart find replacement"
Enter
Wait 500
Type "fd '\.rs$' src/ --max-depth 1"
Enter
Wait 1500

Type "fd test --type f --extension rs"
Enter
Wait 1500

# ── Scene 4: bat — syntax-highlighted file viewing ──
Type "# bat: cat with syntax highlighting + git integration"
Enter
Wait 500
Type "bat src/hints.rs --range 1:30"
Enter
Wait 2500

# ── Scene 5: television — fuzzy file/grep picker (TUI) ──
Type "# television: interactive fuzzy finder with preview"
Enter
Wait 500
Type "tv files"
Enter
Wait 3000
# User would interact here — for scripted demo, send Escape after pause
[Console]::Write("`e")  # Escape
Wait 1000

# ── Scene 6: zoxide — frecency directory jumping ──
Type "# zoxide: smart cd — learns your most-visited directories"
Enter
Wait 500
Type "z psmux"
Enter
Wait 1000
Type "pwd"
Enter
Wait 1000

Type "z src"
Enter
Wait 500
Type "pwd"
Enter
Wait 1500

# ── Scene 7: All together in a psmux workflow ──
Type "# Combine them: fd + bat in a psmux pane"
Enter
Wait 500
Type "fd config --type f | head -5 | xargs bat --style=header,grid"
Enter
Wait 3000

# ── Scene 8: fzf integration ──
Type "# fzf: fuzzy match anything piped to it"
Enter
Wait 500
Type "rg --files | fzf --preview 'bat --color=always {}'"
Enter
Wait 3000
[Console]::Write("`e")  # Escape out of fzf
Wait 1000

# ── Fin ──
Type "# All tools: cargo install or winget install"
Enter
Type "# psmux install.ps1 -Full sets up everything"
Enter
Wait 3000
