# Three-State Pane Focus Borders

**Date:** 2026-03-28
**Branch:** ohboy-builds
**Status:** Approved

## Problem

psmux highlights the active pane with green borders, but this highlight persists even when the user alt-tabs to a browser or another terminal. There's no visual distinction between "I'm working in this pane" and "psmux is in the background." This makes it harder to glance at psmux and know whether it has input focus.

## Design

### Three Visual States

| State | Condition | Border Style | Default |
|---|---|---|---|
| **Active + focused** | Pane is active AND psmux has OS focus | `pane-active-border-style` | `fg=green` (existing) |
| **Inactive + focused** | Pane is not active AND psmux has OS focus | `pane-border-style` | default terminal color (existing) |
| **Unfocused** | psmux lost OS focus (any pane) | `pane-border-unfocused-style` | `fg=darkgray,dim` (new) |

When unfocused, ALL borders collapse to the same muted style — there is no "active" pane distinction because the user isn't interacting with psmux at all.

### New Configuration Option

```
set -g pane-border-unfocused-style "fg=darkgray,dim"
```

Follows tmux naming convention. Parsed by the existing `parse_tmux_style()` function. Users who don't want this behavior set it to match their active style, or leave `focus-events off` (which disables the feature entirely).

### Status Bar Dimming

When unfocused, the status bar also applies `dim` attribute to the entire rendered line. This reinforces the "backgrounded" state without requiring a separate config option.

## Architecture

### State Tracking

Add `window_focused: bool` field to `AppState` in `src/types.rs`. Default: `true` (assume focused on startup).

Toggled by the existing `CtrlReq::FocusIn` / `CtrlReq::FocusOut` handlers in `src/server/mod.rs:4614-4654`. These handlers already fire — they just need to flip the bool in addition to sending CSI sequences to child PTYs.

### Rendering Changes

In `src/rendering.rs:382-484` where border styles are selected:

```
if !app.window_focused:
    style = app.pane_border_unfocused_style
elif pane is active:
    style = app.pane_active_border_style
else:
    style = app.pane_border_style
```

This replaces the current two-way active/inactive lookup with a three-way check. The unfocused check is first — when unfocused, all borders use the same style regardless of which pane is active.

### Status Bar Changes

In `src/app.rs:551-620` where the status bar is rendered, wrap the output with dim attribute when `!app.window_focused`.

### Config Parsing

In `src/types.rs`, add `pane_border_unfocused_style: String` to `AppState` with default `"fg=darkgray,dim"`.

In `src/config.rs`, add parsing for `pane-border-unfocused-style` alongside the existing `pane-border-style` and `pane-active-border-style` options.

In `src/format.rs`, add display support for querying the new option via `show-options`.

### Re-render Trigger

The `FocusIn`/`FocusOut` handlers must trigger a full re-render of the active window after toggling `window_focused`. This ensures borders update immediately when the user alt-tabs. The existing `needs_redraw` flag or equivalent mechanism handles this.

## Interaction with Existing Features

- **`focus-events` option**: When `focus-events` is `off`, the FocusIn/FocusOut handlers still toggle `window_focused` for border rendering. The `focus-events` option only controls whether CSI I/O sequences are forwarded to child PTYs. Border dimming is always active.
- **Hooks**: The existing `pane-focus-in` / `pane-focus-out` hooks continue to fire as before.
- **CustomPaneBackend / remote sessions**: Focus events propagate through the same server path, so remote/backend panes get the same visual treatment.

## Files Modified

| File | Change |
|---|---|
| `src/types.rs` | Add `window_focused: bool`, `pane_border_unfocused_style: String` to `AppState` |
| `src/server/mod.rs` | Toggle `window_focused` in FocusIn/FocusOut handlers, trigger redraw |
| `src/rendering.rs` | Three-way border style selection based on focus state |
| `src/app.rs` | Dim status bar when unfocused |
| `src/config.rs` | Parse `pane-border-unfocused-style` option |
| `src/format.rs` | Display `pane-border-unfocused-style` in show-options |

## Testing

- **Unit test**: Verify three-way style selection logic returns correct style for each state combination
- **Integration test**: Toggle `window_focused` and verify rendered border characters use expected ANSI escape codes
- **Manual test**: Run psmux with split panes, alt-tab away, confirm all borders dim; alt-tab back, confirm active pane border restores to green

## Out of Scope

- Border character swap (dashed lines when unfocused) — can be added later
- Per-pane unfocused styles — all panes use the same unfocused style
- Cursor visibility changes — left to the terminal emulator
