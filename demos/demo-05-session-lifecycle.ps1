# Demo 05: Session Lifecycle — Detach, Reattach, Resurrect
# Shows: multiple sessions, detach/attach, session listing, resurrection

. "$PSScriptRoot/lib-demo.ps1"

Demo-Init -SessionName "lifecycle"

# ── Create sessions with work ──
psmux new-session -d -s backend
Start-Sleep -Milliseconds 800
psmux new-session -d -s frontend
Start-Sleep -Milliseconds 800

psmux send-keys -t backend "echo 'API server running on :8080...'" Enter
Demo-Caption "Create two named sessions with work in progress"
Start-Sleep -Milliseconds 500
psmux send-keys -t frontend "echo 'React dev server on :3000...'" Enter
Demo-Wait 1000

# ── List sessions ──
# Use a third session to drive the demo visually
psmux new-session -d -s lifecycle
Start-Sleep -Milliseconds 800

Demo-Type "psmux ls" -Caption "psmux ls — list all running sessions" -WaitMs 2500

# ── Attach to backend ──
# We'll simulate this by showing the command, then switching
Demo-Type "echo 'Attaching to backend session...'" -Caption "psmux attach -t backend"
Demo-Wait 1000

# Split in backend for visual interest
psmux send-keys -t backend "echo 'Deploying v2.1 to staging...'" Enter
Demo-Wait 500
# Add a split in backend
psmux split-window -t backend 2>$null
Start-Sleep -Milliseconds 500
psmux send-keys -t backend "echo 'tail -f /var/log/api.log'" Enter
Demo-Wait 1000

# ── Show session switching ──
Demo-Type "psmux switch-client -t frontend 2>/dev/null; echo 'Switched to frontend'" `
    -Caption "switch-client — jump between sessions without detaching" -WaitMs 2000

# ── Simulate detach ──
Demo-Type "echo 'Detaching... (Ctrl+b d in real usage)'" `
    -Caption "Ctrl+b d — detach from session (keeps running)" -WaitMs 1500

# ── Sessions still running ──
Demo-Type "psmux ls" -Caption "Sessions persist after detach — reattach anytime" -WaitMs 2500

# ── Session resurrection ──
Demo-Type "echo ''" -WaitMs 200
Demo-Type "echo '=== Session Resurrection ==='" `
    -Caption "Session Resurrection — survive crashes and restarts" -WaitMs 1500

# Kill backend (simulate crash)
Demo-Type "psmux kill-session -t backend" `
    -Caption "kill-session — simulates a crash or restart" -WaitMs 1000

Demo-Type "psmux ls" -Caption "Backend session is gone..." -WaitMs 2000

# Resurrect
Demo-Type "psmux resurrect backend" `
    -Caption "resurrect — restore session from saved snapshot" -WaitMs 2000

Demo-Type "psmux ls" `
    -Caption "Backend is back! Layout and state preserved." -WaitMs 2500

# ── Final message ──
Demo-Type "echo '# Sessions survive: terminal close, RDP disconnect, crashes'" `
    -Caption "No more lost work — psmux has your back"
Demo-Type "echo '# Zero-config. Automatic snapshots. One command to restore.'" `
    -Caption "Zero config. Automatic snapshots. One command to restore."

Demo-SaveCaptions "$PSScriptRoot/session-lifecycle.srt"

Write-Host "Attaching to lifecycle session..." -ForegroundColor Cyan
Demo-Wait 500

# Attach to lifecycle (the session showing the commands)
$script:Session = "lifecycle"
Demo-Attach
