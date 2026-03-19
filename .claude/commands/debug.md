---
description: Help debug a psmux runtime issue
---

Debug this psmux issue: $ARGUMENTS

Debugging approach:
1. Identify the likely subsystem (rendering, input, IPC, session management, configuration)
2. Search the source code for the relevant code paths
3. Look for common pitfalls:
   - Console mode not set correctly (VT processing disabled)
   - Named pipe connection race conditions
   - Buffer size mismatches in console API calls
   - Key event parsing differences between terminals
   - Pane geometry calculation off-by-one errors
4. Suggest targeted logging/debug output to narrow down the issue
5. Propose a fix with explanation
6. Add a regression test if applicable

If the issue description is vague, ask clarifying questions about:
- Which terminal emulator (Windows Terminal, cmd.exe, PowerShell, ConEmu)?
- Windows version?
- PowerShell version?
- Exact steps to reproduce?
- Expected vs actual behavior?
