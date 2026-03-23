#!/bin/bash
# Claude Code hook: auto-capture pane output when an agent finishes
# Triggered by: Stop event (agent response complete)
# Env vars: $CLAUDE_SESSION_ID, $TMUX_PANE
#
# Captures the active pane's visible content + scrollback to a timestamped
# log file. Useful for post-mortem analysis of agent swarm runs.

LOG_DIR="${USERPROFILE:-$HOME}/.psmux/agent-logs"
mkdir -p "$LOG_DIR"

TIMESTAMP=$(date +%Y%m%d-%H%M%S)
SESSION_ID="${CLAUDE_SESSION_ID:-unknown}"
PANE_ID="${TMUX_PANE:-$(psmux display-message -p '#{pane_id}' 2>/dev/null)}"

if [ -z "$PANE_ID" ]; then
    exit 0
fi

LOG_FILE="$LOG_DIR/${TIMESTAMP}_${SESSION_ID}_${PANE_ID}.log"

# Capture full scrollback (-S -10000 to get last 10k lines)
psmux capture-pane -t "$PANE_ID" -p -S -10000 > "$LOG_FILE" 2>/dev/null

# Only keep if non-empty
if [ ! -s "$LOG_FILE" ]; then
    rm -f "$LOG_FILE"
    exit 0
fi

# Notify via status bar
LINE_COUNT=$(wc -l < "$LOG_FILE")
psmux display-message "Agent output captured: ${LINE_COUNT} lines → ${LOG_FILE##*/}" 2>/dev/null
