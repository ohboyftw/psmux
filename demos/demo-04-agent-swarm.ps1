# Demo 04: Agent Swarm — AI Multi-Agent Orchestration
# Shows: JSON layout, multi-pane spawn, capture-pane, wait-pane,
#        synchronize-panes, agent metadata
#
# Short (README): 15s — layout spawn + capture-pane
# Full (social): 40s — full agent workflow with monitoring

$delay = 50

function Type($text) { foreach ($c in $text.ToCharArray()) { [Console]::Write($c); Start-Sleep -Milliseconds $delay } }
function Enter { [Console]::Write("`r`n"); Start-Sleep -Milliseconds 300 }
function Wait($ms) { Start-Sleep -Milliseconds $ms }
function Pause { Start-Sleep -Milliseconds 1500 }

# ── Scene 1: Create agent workspace ──
Wait 500
Type "psmux new-session -s swarm"
Enter
Wait 2000

# ── Scene 2: Create a multi-pane layout for agents ──
Type "# Create a 3-pane agent workspace"
Enter
Wait 500

# Split for agent 1
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("%")
Wait 1000

# Split bottom for agent 2
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write('"')
Wait 1000

# ── Scene 3: Label the panes with agent roles ──
# Go to first pane
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("0")
Wait 500
Type "echo '=== Coordinator Agent ==='"
Enter
Wait 500

# Second pane
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("1")
Wait 500
Type "echo '=== Research Agent ==='"
Enter
Wait 500

# Third pane
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("2")
Wait 500
Type "echo '=== Code Agent ==='"
Enter
Wait 800

# ── Scene 4: Simulate agent work with send-keys ──
Type "# Orchestrate via CLI — send commands to any pane"
Enter
Wait 500

# Back to coordinator
[Console]::Write("`u{0002}")
Wait 200
[Console]::Write("0")
Wait 500

Type "psmux send-keys -t %1 'rg TODO --type rust --count' Enter"
Enter
Wait 2000

Type "psmux send-keys -t %2 'fd test --extension rs' Enter"
Enter
Wait 2000

# ── Scene 5: Capture output from agent panes ──
Type "# Capture agent output programmatically"
Enter
Wait 500
Type "psmux capture-pane -t %1 -p | tail -5"
Enter
Wait 2000

# ── Scene 6: Synchronize panes ──
Type "psmux set -g synchronize-panes on"
Enter
Wait 800
Type "echo 'All panes receive this!'"
Enter
Wait 2000
Type "psmux set -g synchronize-panes off"
Enter
Wait 1500

# ── Scene 7: Show list-panes with metadata ──
Type "psmux list-panes -F '#{pane_index}: #{pane_title} [#{pane_width}x#{pane_height}]'"
Enter
Wait 2000

# ── Scene 8: JSON output for agent monitoring ──
Type "psmux list-panes --json"
Enter
Wait 2500

# ── Fin ──
Type "# psmux: the tmux backend for Claude Code agent teams on Windows"
Enter
Wait 3000
