#!/bin/bash
# Claude Code hook: tag pane with agent metadata when TeammateTool spawns
# Triggered by: PostToolUse for Bash/Write tools (when teammate spawns)
# Env vars: $CLAUDE_TOOL_NAME, $TMUX_PANE
#
# Sets @agent metadata on the pane for queryable swarm status.

PANE_ID="${TMUX_PANE:-}"
TOOL_NAME="${CLAUDE_TOOL_NAME:-}"

if [ -z "$PANE_ID" ]; then
    exit 0
fi

# Tag the pane with the spawning agent's identity
psmux set-option -t "$PANE_ID" -p @agent "${TOOL_NAME:-claude-code}" 2>/dev/null
psmux set-option -t "$PANE_ID" -p @spawned "$(date +%Y%m%d-%H%M%S)" 2>/dev/null

# Save resurrection snapshot (structural change: new agent pane)
# This is belt-and-suspenders — the server also saves on split-window,
# but the hook fires even for agent panes created outside psmux's CtrlReq path.
