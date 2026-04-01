# Demo 04: Agent Swarm — AI Multi-Agent Orchestration
# All pane management via direct CLI. Shell commands via send-keys -l.

. "$PSScriptRoot/lib-demo.ps1"

Demo-Init -SessionName "swarm"

Demo-Run "new-session -d -s swarm" -WaitMs 1500

# ── Build 3-pane workspace ──
Demo-ShellCmd "echo === Coordinator ===" -Caption "Create agent workspace — coordinator pane"

Demo-Run "split-window -h -t swarm" -Caption "Split for research agent" -WaitMs 800
Demo-ShellCmd "echo === Research Agent ===" -WaitMs 500

Demo-Run "split-window -v -t swarm" -Caption "Split for code agent" -WaitMs 800
Demo-ShellCmd "echo === Code Agent ===" -WaitMs 500

# ── Focus coordinator (pane 0) ──
Demo-Run "select-pane -t swarm:0.0" -Caption "Focus coordinator pane" -WaitMs 800

# ── Dispatch to agents ──
Demo-ShellCmd "echo Dispatching tasks to agents..." -Caption "Orchestrate agents via CLI"

# Send command to research pane (pane 1)
& psmux send-keys -t "swarm:0.1" -l -- "rg TODO --type rust --count"
& psmux send-keys -t "swarm:0.1" Enter
Demo-Caption "send-keys -t %1: research agent searches TODOs"
Demo-Wait 2000

# Send command to code pane (pane 2)
& psmux send-keys -t "swarm:0.2" -l -- "fd test --extension rs"
& psmux send-keys -t "swarm:0.2" Enter
Demo-Caption "send-keys -t %2: code agent finds test files"
Demo-Wait 2000

# ── Capture output ──
Demo-ShellCmd "psmux capture-pane -t %1 -p | tail -5" `
    -Caption "capture-pane — read agent output programmatically" -WaitMs 2500

# ── Synchronize panes ──
Demo-Run "set -g synchronize-panes on" `
    -Caption "synchronize-panes on — [SYNC] broadcast to ALL panes" -WaitMs 800

Demo-ShellCmd "echo All agents see this" `
    -Caption "Every pane receives the same input" -WaitMs 2000

Demo-Run "set -g synchronize-panes off" `
    -Caption "synchronize-panes off — back to single pane" -WaitMs 1000

# ── JSON output ──
Demo-Run "select-pane -t swarm:0.0" -WaitMs 300
Demo-ShellCmd "psmux list-panes --json" `
    -Caption "list-panes --json — structured output for monitoring" -WaitMs 2500

# ── Final ──
Demo-ShellCmd "echo CustomPaneBackend: JSON-RPC for Claude Code agent teams" `
    -Caption "Built-in Claude Code TeammateTool backend"

Demo-SaveCaptions "$PSScriptRoot/agent-swarm.srt"
Demo-Attach -Target "swarm"
