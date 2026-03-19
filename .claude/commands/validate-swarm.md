---
description: Run the psmux swarm backend validation suite
allowed-tools: Bash, Read
---

Run the swarm backend validation test suite to verify psmux works as a
Claude Code TeammateTool tmux backend.

Execute `pwsh tests/validate-swarm-backend.ps1` and report results.

If any tests fail:
1. Identify the specific failure from the test output
2. Look up the corresponding fix in `.claude/skills/swarm-backend-validation/SKILL.md`
   under the "What to Do With Failures" table
3. Search the psmux source for the relevant code path
4. Suggest a fix

If all tests pass, confirm psmux is validated as a swarm backend and
suggest trying `/spawn-swarm` with a small task to test end-to-end.
