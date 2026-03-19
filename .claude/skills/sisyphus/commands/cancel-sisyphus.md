---
description: "Cancel an active Sisyphus iteration loop"
allowed-tools: ["Bash(test -f .claude/sisyphus.local.md:*)", "Bash(rm .claude/sisyphus.local.md)", "Read(.claude/sisyphus.local.md)"]
hide-from-slash-command-tool: "true"
---

# Cancel Sisyphus Loop

Stop the active Sisyphus iteration loop by removing the state file.

**Steps:**

1. Check if a Sisyphus loop is currently active:
   - Run: `test -f .claude/sisyphus.local.md && echo "ACTIVE" || echo "INACTIVE"`

2. If ACTIVE, read the state file to show the user what's being cancelled:
   - Read `.claude/sisyphus.local.md` to display current iteration and prompt

3. Remove the state file to deactivate the loop:
   - Run: `rm .claude/sisyphus.local.md`

4. Confirm to the user:
   - "Sisyphus loop cancelled. The session will exit normally after the current task completes."
   - Summarize what was accomplished across iterations if possible

5. If INACTIVE, inform the user:
   - "No active Sisyphus loop found. Nothing to cancel."
