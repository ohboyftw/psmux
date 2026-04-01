# Demo 04: Agent Swarm — AI Multi-Agent Orchestration
# Shows: multi-pane layout, send-keys, capture-pane, synchronize-panes, JSON output

. "$PSScriptRoot/lib-demo.ps1"

Demo-Init -SessionName "swarm"

psmux new-session -d -s swarm
Start-Sleep -Milliseconds 1500

# ── Build 3-pane agent workspace ──
Demo-Type "echo '=== Coordinator ==='" -Caption "Create agent workspace — coordinator pane"

Demo-Prefix "%" -Caption "Split for research agent"
Demo-Type "echo '=== Research Agent ==='" -WaitMs 500

Demo-Prefix '"' -Caption "Split for code agent"
Demo-Type "echo '=== Code Agent ==='" -WaitMs 500

# ── Focus coordinator ──
psmux send-keys -t swarm:0.0 "" # ensure pane 0 is selectable
Demo-Prefix "0" -Caption "Focus coordinator pane"
Demo-Wait 500

# ── Dispatch commands to agents via send-keys ──
Demo-Type "echo 'Dispatching tasks to agents...'" -Caption "Orchestrate agents via CLI" -WaitMs 800

psmux send-keys -t swarm:0.1 "rg TODO --type rust --count" Enter
Demo-Caption "send-keys -t %1: research agent searches TODOs"
Demo-Wait 2000

psmux send-keys -t swarm:0.2 "fd test --extension rs" Enter
Demo-Caption "send-keys -t %2: code agent finds test files"
Demo-Wait 2000

# ── Capture output ──
Demo-Type "psmux capture-pane -t %1 -p | tail -5" `
    -Caption "capture-pane -t %1 — read agent output programmatically" -WaitMs 2500

# ── Synchronize panes ──
Demo-Type "psmux set -g synchronize-panes on" `
    -Caption "synchronize-panes on — broadcast input to ALL panes" -WaitMs 1000

Demo-Type "echo 'All agents see this command'" `
    -Caption "[SYNC] indicator appears — every pane receives input" -WaitMs 2000

Demo-Type "psmux set -g synchronize-panes off" `
    -Caption "synchronize-panes off — back to single-pane input" -WaitMs 1000

# ── JSON output for monitoring ──
Demo-Type "psmux list-panes --json" `
    -Caption "list-panes --json — structured output for agent monitoring" -WaitMs 2500

# ── show wait-pane ──
Demo-Type "echo '# wait-pane -S ready: block until agent signals readiness'" `
    -Caption "wait-pane: server-side readiness polling for agent spawn"

Demo-Type "echo '# CustomPaneBackend: JSON-RPC for Claude Code TeammateTool'" `
    -Caption "Built-in Claude Code agent teams backend"

Demo-SaveCaptions "$PSScriptRoot/agent-swarm.srt"

Write-Host "Attaching..." -ForegroundColor Cyan
Demo-Wait 500
Demo-Attach
