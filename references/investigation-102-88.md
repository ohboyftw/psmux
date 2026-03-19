# Investigation: Issues #102 and #88

## Issue #102: Multiple Input Handling Issues

### 1. Resize Hang
- **Root Cause**: `Event::Resize` handler (`app.rs:1067-1078`) resizes only the active pane to full terminal dimensions (wrong). Rendering path corrects it, causing double-resize ConPTY output storm.
- **Fix**: Replace handler body with `resize_all_panes(&mut app)` (already exists in `tree.rs:213-269`). Risk: LOW.

### 2. Alt Key Consumed
- **Root Cause**: In Prefix mode, unrecognized keys are silently dropped during `escape_time_ms` window (`input.rs:330-331`). Alt+key after accidental prefix press is eaten.
- **Fix**: Forward unrecognized keys to active pane when escape_time expires instead of dropping. Risk: MEDIUM.

### 3. Mouse Behavior
- **Root Cause**: `pane_wants_mouse()` (`window_ops.rs:190-204`) returns true when PSReadLine spuriously enables AnyMotion mouse tracking (DECSET 1003). Breaks right-click paste at shell prompts.
- **Fix**: Check `mouse_protocol_mode()` directly for right-click, not the combined helper. Risk: MEDIUM-HIGH.

## Issue #88: Codex CLI Scrolling

- **Root Cause**: Codex uses scroll regions (CSI r / DECSTBM) on the PRIMARY screen, not alternate screen. `alternate_screen()` returns false, so psmux enters copy mode on scroll — but scrollback is empty because `grid.rs:566` discards rows when scroll region is active.
- **Fix**: In scroll handlers (`input.rs:1906-1912`), also check `scroll_region_active()`. If true, forward scroll events instead of entering copy mode. Risk: LOW-MEDIUM.

## Recommended Fix Priority
1. Resize hang (easiest, lowest risk)
2. Codex scrolling (clear root cause)
3. Alt key consumed (needs escape_time analysis)
4. Mouse behavior (most complex)
