---
description: Check which tmux commands are implemented vs missing
---

Analyze the psmux source code and compare against the standard tmux command set. Produce a compatibility report:

1. **Scan the source** for all implemented command handlers/subcommands
2. **Compare against the core tmux commands**: new-session, kill-session, has-session, list-sessions, rename-session, attach-session, detach-client, new-window, kill-window, select-window, next-window, previous-window, last-window, list-windows, rename-window, move-window, link-window, unlink-window, split-window, select-pane, kill-pane, resize-pane, swap-pane, rotate-window, break-pane, join-pane, display-panes, send-keys, capture-pane, copy-mode, paste-buffer, set-buffer, show-buffer, list-buffers, delete-buffer, choose-buffer, display-message, set-option, show-options, bind-key, unbind-key, list-keys, source-file, run-shell, if-shell, pipe-pane, clock-mode
3. **Report results** in three categories:
   - ✅ Implemented
   - ⚠️ Partially implemented (note what's missing)
   - ❌ Not implemented

If $ARGUMENTS is provided, focus the analysis on that specific command.
