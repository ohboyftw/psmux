---
name: fix-windows-compat
description: Debug and fix Windows-specific issues in psmux — console API problems, VT100 rendering glitches, named pipe IPC failures, terminal emulator compatibility, and PowerShell/cmd.exe differences. Use when the user reports rendering bugs, garbled output, broken key input, session attach/detach failures, or mentions Windows Terminal, ConEmu, cmd.exe, PowerShell compatibility issues. Also trigger for "doesn't work on Windows 10", "rendering broken", "keys not working", "session won't attach".
---

# Fixing Windows Compatibility Issues in psmux

## Common Issue Categories

### 1. Console/VT100 Rendering
psmux uses Virtual Terminal Sequences for rendering. Common issues:

**Enable VT processing** — Must call `SetConsoleMode` with `ENABLE_VIRTUAL_TERMINAL_PROCESSING`:
```rust
// Ensure this is set on stdout handle
let mode = current_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING;
SetConsoleMode(handle, mode);
```

**Terminal detection** — Different terminals support different VT levels:
- Windows Terminal: Full VT100/xterm-256color support
- PowerShell 7+: Good VT support
- cmd.exe: Basic VT, may need fallbacks
- ConEmu: Own escape sequence extensions
- Legacy PowerShell 5.1: Limited VT support

**Debugging rendering**: Add `--debug` or environment variable to dump raw escape sequences to a log file.

### 2. Input Handling
Windows console input comes through `ReadConsoleInput` which returns `KEY_EVENT_RECORD`:

- **Ctrl+key combinations**: `dwControlKeyState` flags distinguish Ctrl, Alt, Shift
- **Arrow keys / special keys**: Check `wVirtualKeyCode` (VK_UP, VK_DOWN, etc.)
- **Mouse events**: `MOUSE_EVENT_RECORD` for resize and click handling
- **Prefix key timing**: Buffer the prefix key and wait for the next input with a timeout

Common pitfall: Some terminals send different key codes. Always test with Windows Terminal AND cmd.exe.

### 3. Named Pipe IPC
Session detach/attach uses Windows named pipes (`\\.\pipe\psmux-session-name`):

**Common failures:**
- Pipe already exists (stale session) → Check if pipe is orphaned, clean up
- Access denied → Pipe security descriptor needs correct permissions
- Broken pipe on detach → Handle `ERROR_BROKEN_PIPE` gracefully
- Connection timeout → Use `WaitNamedPipe` with reasonable timeout

**Debugging IPC:**
```rust
// Log pipe operations
eprintln!("[IPC] Creating pipe: {}", pipe_name);
eprintln!("[IPC] Client connected");
eprintln!("[IPC] Received {} bytes", n);
```

### 4. Process Spawning
Panes spawn child processes (PowerShell, cmd.exe, etc.):

- Use `CreateProcessW` with `STARTUPINFOW` configured for the pseudo-console
- ConPTY (`CreatePseudoConsole`) is the modern approach for Windows 10 1809+
- Fallback for older builds may need traditional console allocation
- Shell detection: Check `$env:SHELL`, `COMSPEC`, or default to `cmd.exe`

### 5. Screen Buffer & Resize
- `GetConsoleScreenBufferInfo` for current dimensions
- `WINDOW_BUFFER_SIZE_EVENT` for resize notifications
- Coordinate systems: Windows uses (X, Y) with (0,0) at top-left — same as VT

### Debugging Workflow
1. Reproduce the issue — note exact terminal emulator, Windows version, PowerShell version
2. Check `cargo clippy` for any warnings related to the area
3. Add targeted logging around the suspected code path
4. Test on both Windows Terminal and cmd.exe
5. Test on both Windows 10 and Windows 11 if possible
6. Run `cargo test` to ensure no regressions

### Windows Version Differences
- **Windows 10 1607+**: Basic VT support
- **Windows 10 1809+**: ConPTY API available (preferred for pseudo-console)
- **Windows 11**: Improved VT and ConPTY support
- Always check `GetVersionEx` or feature-detect rather than hard-coding version checks
