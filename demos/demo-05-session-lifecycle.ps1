# Demo 05: Session Lifecycle — Detach, Reattach, Resurrect
# All management via direct CLI commands.

. "$PSScriptRoot/lib-demo.ps1"

Demo-Init -SessionName "lifecycle"

# ── Create sessions with work ──
Demo-Run "new-session -d -s backend" -WaitMs 800
Demo-Run "new-session -d -s frontend" -WaitMs 800

& psmux send-keys -t backend -l -- "echo API server running on :8080"
& psmux send-keys -t backend Enter
Demo-Caption "Create two named sessions with work in progress"
Demo-Wait 500

& psmux send-keys -t frontend -l -- "echo React dev server on :3000"
& psmux send-keys -t frontend Enter
Demo-Wait 800

# ── Create a driver session to show commands ──
Demo-Run "new-session -d -s lifecycle" -WaitMs 800

# ── List sessions ──
Demo-ShellCmd "psmux ls" -Caption "psmux ls — list all running sessions" -WaitMs 2500

# ── Add a split in backend for visual interest ──
Demo-Run "split-window -h -t backend" -WaitMs 500
& psmux send-keys -t backend -l -- "echo tail -f server.log"
& psmux send-keys -t backend Enter
Demo-Wait 500

# ── Show attaching ──
Demo-ShellCmd "echo Attach with: psmux attach -t backend" `
    -Caption "psmux attach -t backend — reattach to any session" -WaitMs 1500

# ── Show detaching ──
Demo-ShellCmd "echo Detach with: Ctrl+b d (session keeps running)" `
    -Caption "Ctrl+b d — detach. Session keeps running in background." -WaitMs 1500

# ── Sessions persist ──
Demo-ShellCmd "psmux ls" -Caption "Sessions persist after detach — reattach anytime" -WaitMs 2000

# ── Resurrection ──
Demo-ShellCmd "echo === Session Resurrection ===" `
    -Caption "Session Resurrection — survive crashes and restarts" -WaitMs 1500

# Kill backend (simulate crash)
Demo-Run "kill-session -t backend" -Caption "kill-session — simulates a crash" -WaitMs 1000

Demo-ShellCmd "psmux ls" -Caption "Backend session is gone..." -WaitMs 2000

# Resurrect
Demo-ShellCmd "psmux resurrect backend" `
    -Caption "resurrect — restore session from saved snapshot" -WaitMs 2000

Demo-ShellCmd "psmux ls" -Caption "Backend is back! Layout preserved." -WaitMs 2500

# ── Final ──
Demo-ShellCmd "echo Sessions survive: terminal close, RDP disconnect, crashes" `
    -Caption "No more lost work — psmux has your back"

Demo-SaveCaptions "$PSScriptRoot/session-lifecycle.srt"
Demo-Attach -Target "lifecycle"
