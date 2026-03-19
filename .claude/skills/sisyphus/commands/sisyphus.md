---
description: "Start a Sisyphus iterative development loop in current session"
argument-hint: "PROMPT [--max-iterations N] [--completion-promise TEXT]"
allowed-tools: ["Bash(python3 ${CLAUDE_PLUGIN_ROOT}/scripts/setup_sisyphus.py:*)"]
hide-from-slash-command-tool: "true"
---

# Sisyphus Loop Command

Initialize an iterative development loop. Claude will work on the task, attempt to exit,
and be given the same prompt again to iterate on prior work visible in files and git history.

Execute the setup script with all arguments:

```!
python3 "${CLAUDE_PLUGIN_ROOT}/scripts/setup_sisyphus.py" $ARGUMENTS
```

After the loop is initialized, begin working on the task described in the prompt.

**ITERATION RULES:**
- Each iteration, review your prior work (files, git log, test results) before making changes
- Build incrementally — don't rewrite everything each iteration
- If tests exist, run them to assess current state before modifying code
- Commit meaningful progress at each iteration

**CRITICAL RULE:** If a completion promise is set, you may ONLY output it wrapped in
`<promise>` XML tags when the statement is completely and unequivocally TRUE.
Do NOT output false statements to escape the loop. Do NOT lie even if stuck.
If blocked, document what's preventing progress and continue trying.
