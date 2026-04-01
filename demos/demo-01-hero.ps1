# Demo 01: Hero — psmux in 30 seconds
# Shows: session creation, pane splitting, navigation, status bar, theming
#
# Short version (README GIF): first 12s — session + splits + navigate
# Full version (social): all 30s — includes resize, zoom, rename, detach tease

$delay = 60  # ms between keystrokes (typing speed)

function Type($text) { foreach ($c in $text.ToCharArray()) { [Console]::Write($c); Start-Sleep -Milliseconds $delay } }
function Enter { [Console]::Write("`r`n"); Start-Sleep -Milliseconds 300 }
function Wait($ms) { Start-Sleep -Milliseconds $ms }
function Pause { Start-Sleep -Milliseconds 1500 }

# ── Scene 1: Create a named session ──
Wait 500
Type "psmux new-session -s work"
Enter
Wait 2000

# ── Scene 2: Show we're inside psmux ──
Type "echo 'Welcome to psmux — tmux for Windows'"
Enter
Pause

# ── Scene 3: Split panes ──
# Ctrl+b % = vertical split
[Console]::Write("`u{0002}")  # Ctrl+B
Wait 200
[Console]::Write("%")
Wait 1500

# Type something in right pane
Type "rg --version"
Enter
Wait 800

# Ctrl+b " = horizontal split
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write('"')
Wait 1500

# Type in bottom-right pane
Type "fd --version"
Enter
Wait 800

# ── Scene 4: Navigate between panes ──
# Ctrl+b Left = go left
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("`e[D")  # Left arrow
Wait 800

Type "bat --version"
Enter
Wait 800

# ── Scene 5: Zoom a pane ──
# Ctrl+b z = toggle zoom
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("z")
Wait 2000

# Unzoom
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("z")
Wait 1500

# ── Scene 6: Rename window ──
# Ctrl+b , = rename
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write(",")
Wait 500
Type "dev"
Enter
Wait 1500

# ── Scene 7: Create second window ──
# Ctrl+b c = new window
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("c")
Wait 1500
Type "echo 'Window 2 — monitoring'"
Enter
Wait 1000

# Switch back to window 0
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("0")
Wait 2000

# ── Fin ──
Type "# psmux: 92 tmux commands, native Windows, zero WSL"
Enter
Wait 3000
