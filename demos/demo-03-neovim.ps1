# Demo 03: Neovim in psmux — Full TUI Support
# Shows: mouse clicks, cursor shape tracking, focus events, bracketed paste,
#        pane title bars, split navigation
#
# Short (README): 15s — split + nvim + pane switch with cursor change
# Full (social): 35s — full editing workflow across panes

$delay = 50

function Type($text) { foreach ($c in $text.ToCharArray()) { [Console]::Write($c); Start-Sleep -Milliseconds $delay } }
function Enter { [Console]::Write("`r`n"); Start-Sleep -Milliseconds 300 }
function Wait($ms) { Start-Sleep -Milliseconds $ms }
function Pause { Start-Sleep -Milliseconds 1500 }

# ── Scene 1: Open psmux with title bars ──
Wait 500
Type "psmux new-session -s nvim-demo"
Enter
Wait 2000

# Enable pane title bars
Type "psmux set -g pane-border-status top"
Enter
Wait 1000

# ── Scene 2: Open neovim ──
Type "nvim src/hints.rs"
Enter
Wait 2500

# ── Scene 3: Navigate in neovim — cursor is block (normal mode) ──
# Move down with j
Type "jjjjjjjjjj"
Wait 1500

# Search for a function
Type "/fn scan"
Enter
Wait 1500

# ── Scene 4: Enter insert mode — cursor changes to bar ──
Type "i"
Wait 1500
# Type a comment
Type "// mouse + focus events work in psmux!"
Wait 1000
# Back to normal mode — cursor changes to block
[Console]::Write("`e")  # Escape
Wait 1500

# Undo the edit
Type "u"
Wait 1000

# ── Scene 5: Split pane while nvim is running ──
# Ctrl+b % = vertical split
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("%")
Wait 1500

# Right pane: open another file in nvim
Type "nvim src/rendering.rs"
Enter
Wait 2000

# ── Scene 6: Switch panes — focus event triggers ──
# This sends FocusOut to right nvim, FocusIn to left nvim
# Left nvim's :checktime triggers, cursor style restores
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("`e[D")  # Left arrow
Wait 2000

# Back to right pane
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("`e[C")  # Right arrow
Wait 1500

# ── Scene 7: Visual mode selection (mouse would work here too) ──
Type "V"
Wait 500
Type "jjj"
Wait 1000
# Yank selection
Type "y"
Wait 1000

# ── Scene 8: Quit both editors ──
Type ":q!"
Enter
Wait 500

[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("`e[D")
Wait 500
Type ":q!"
Enter
Wait 1000

# ── Fin ──
Type "# neovim in psmux: mouse, cursor shapes, focus events, bracketed paste"
Enter
Type "# The only Windows multiplexer where nvim works correctly"
Enter
Wait 3000
