#!/bin/bash
# Claude Code hook: display notification in psmux status bar
# Triggered by: Notification event
# Env vars: $CLAUDE_NOTIFICATION (message text)

MSG="${CLAUDE_NOTIFICATION:-$1}"
if [ -z "$MSG" ]; then
    exit 0
fi

# Truncate long messages for status bar
if [ ${#MSG} -gt 80 ]; then
    MSG="${MSG:0:77}..."
fi

# Display in psmux status bar (visible for display-time, default 750ms)
psmux display-message "$MSG" 2>/dev/null
