# Demo 05: Session Lifecycle — Detach, Reattach, Resurrect
# Shows: detach/attach, session listing, session resurrection,
#        multiple named sessions, session switching
#
# Short (README): 15s — detach + list + reattach
# Full (social): 40s — full lifecycle including resurrection

$delay = 50

function Type($text) { foreach ($c in $text.ToCharArray()) { [Console]::Write($c); Start-Sleep -Milliseconds $delay } }
function Enter { [Console]::Write("`r`n"); Start-Sleep -Milliseconds 300 }
function Wait($ms) { Start-Sleep -Milliseconds $ms }
function Pause { Start-Sleep -Milliseconds 1500 }

# ── Scene 1: Create sessions with work in progress ──
Wait 500
Type "# Create two named sessions with work in progress"
Enter
Wait 500

Type "psmux new-session -d -s backend"
Enter
Wait 1000
Type "psmux send-keys -t backend 'echo Working on API server...' Enter"
Enter
Wait 500

Type "psmux new-session -d -s frontend"
Enter
Wait 1000
Type "psmux send-keys -t frontend 'echo Building React components...' Enter"
Enter
Wait 500

# ── Scene 2: List all sessions ──
Type "# List all running sessions"
Enter
Wait 300
Type "psmux ls"
Enter
Wait 2000

# ── Scene 3: Attach to backend ──
Type "psmux attach -t backend"
Enter
Wait 2000

# Do some work
Type "echo 'Deploying v2.1...'"
Enter
Wait 800

# Split a pane for monitoring
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("%")
Wait 1000
Type "echo 'tail -f server.log'"
Enter
Wait 1000

# ── Scene 4: Detach ──
# Ctrl+b d
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("d")
Wait 1500

# ── Scene 5: Switch to frontend session ──
Type "psmux attach -t frontend"
Enter
Wait 2000

# Do work in frontend
Type "echo 'npm run build -- --watch'"
Enter
Wait 1000

# Detach again
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("d")
Wait 1500

# ── Scene 6: Session listing shows both ──
Type "psmux ls"
Enter
Wait 2000

# ── Scene 7: Session resurrection demo ──
Type "# Session resurrection — survive crashes and restarts"
Enter
Wait 500

# Kill the backend session (simulates crash)
Type "psmux kill-session -t backend"
Enter
Wait 1000

# Show sessions — backend is gone
Type "psmux ls"
Enter
Wait 1500

# Resurrect it
Type "psmux resurrect backend"
Enter
Wait 2000

# Verify it's back
Type "psmux ls"
Enter
Wait 2000

# ── Scene 8: Reattach to the resurrected session ──
Type "psmux attach -t backend"
Enter
Wait 2000

Type "echo 'Session restored! Layout and state preserved.'"
Enter
Wait 2000

# Detach for clean exit
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("d")
Wait 1000

# ── Fin ──
Type "# Sessions persist across terminal closes, RDP disconnects, and crashes"
Enter
Type "# No more lost work — psmux has your back"
Enter
Wait 3000
