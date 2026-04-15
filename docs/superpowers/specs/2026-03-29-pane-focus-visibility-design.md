# Pane Focus Visibility — Design Spec

**Date:** 2026-03-29
**Branch:** ohboy-builds
**Status:** Approved

## Problem

The active pane in psmux is hard to distinguish visually, especially:
- Single-pane windows have no border at all — nothing to style
- Multi-pane border color difference (green vs default gray) is subtle
- Status bar dimming on window unfocus is barely noticeable

## Solution: Three Features

### 1. Pane Frame & Title Bar

Every pane gets a **frame** with a title bar showing pane metadata.

#### Layout Model

**Single-pane window** (Leaf at root): Full box border — top title bar row, left/right 1-col borders, bottom 1-row border. PTY content area is `(width-2, height-2)`.

```
┌─ 0: bash ───────────────────────────────────────┐
│                                                   │
│  PTY content area (width-2, height-2)             │
│                                                   │
└───────────────────────────────────────────────────┘
```

**Multi-pane window** (Split): Each pane gets a 1-row title bar at top of its allocated rect. No left/right/bottom borders — existing separator lines handle inter-pane boundaries. PTY content area is `(width, height-1)`.

```
── 0: bash ─────────────────────────────────────────
 PTY content (height-1)

─── separator line (existing) ──────────────────────
── 1: vim ──────────────────────────────────────────
 PTY content (height-1)
```

#### Title Bar Rendering

- Title text is left-aligned within the bar
- Rest of the bar filled with `─` characters
- Styled with the pane's applicable border style:
  - Active pane + window focused: `pane-active-border-style`
  - Inactive pane + window focused: `pane-border-style`
  - Window unfocused: `pane-border-unfocused-style`
- For single-pane box: corners use `┌┐└┘`, sides use `│`

#### Config Options

| Option | Values | Default | Description |
|--------|--------|---------|-------------|
| `pane-border-status` | `top`, `bottom`, `off` | `top` | Title bar position or disable |
| `pane-border-format` | format string | `"#{pane_index}: #{pane_title}"` | Title bar content |

#### Implementation Touch Points

- **`src/rendering.rs` — `render_node()` Leaf case**: Subtract title row from content area before rendering PTY. Render title bar in the reserved row. For root-level Leaf (single pane), additionally draw left/right column borders and bottom row border.
- **`src/rendering.rs` — `fix_border_intersections()`**: Extend to handle box corner characters (`┌┐└┘`) for single-pane frames.
- **`src/types.rs` — `AppState`**: Add `pane_border_status: String` and `pane_border_format: String` fields.
- **`src/config.rs`**: Parse `pane-border-status` and `pane-border-format` as first-class options (promote from `user_options`).
- **`src/server/options.rs`**: Register for `get`/`set` via `show-options`/`set-option`.
- **`src/format.rs`**: `expand_format()` takes `&AppState` but title bar needs per-pane context. Either: (a) add an optional `&Pane` parameter to `expand_format`, or (b) pre-substitute `#{pane_index}` and `#{pane_title}` before calling `expand_format`. Approach (b) is simpler — do string replacement for pane-specific variables first, then pass through the format engine for global variables like `#{session_name}`.
- **`split_with_gaps()`**: Unchanged — title row is *inside* each pane's rect, not an extra gap.

#### `pane-border-status bottom` Clarification

When set to `bottom`:
- **Multi-pane**: title bar is the last row of each pane's rect (PTY gets top rows)
- **Single-pane**: bottom border row of the box carries the title text (`└─ 0: bash ──┘`), top border is plain (`┌──────────┐`)

### 2. Enhanced Status Bar Focus Styling

When the terminal window loses OS focus, the status bar changes dramatically rather than just dimming.

#### Behavior

1. New config option: `status-unfocused-style` (default: empty string)
2. When `window_focused == false`:
   - If `status-unfocused-style` is set (non-empty): use it as the entire status bar base style
   - If empty (default): auto-desaturate the `status-style` colors to grayscale + apply `DIM`
3. Auto-desaturation:
   - RGB colors: luminance-weighted grayscale `(0.299*R + 0.587*G + 0.114*B)`
   - Named colors: map to gray equivalents (green -> darkgray, blue -> darkgray, etc.)
   - Apply `Modifier::DIM` on top

#### Config

```bash
# Explicit override
set -g status-unfocused-style "bg=#333333,fg=#888888"

# Auto-desaturate (default — leave empty)
set -g status-unfocused-style ""
```

#### Implementation Touch Points

- **`src/app.rs` lines ~685-689**: Replace `base_status_style.add_modifier(Modifier::DIM)` with new logic: check `status_unfocused_style`, apply explicit style or auto-desaturate.
- **`src/types.rs` — `AppState`**: Add `status_unfocused_style: String` field.
- **`src/style.rs`**: Add `desaturate_style(style: Style) -> Style` helper — converts fg/bg colors to grayscale, adds DIM.
- **`src/config.rs`**: Parse `status-unfocused-style`.
- **`src/server/options.rs`**: Register for get/set.

### 3. Feature Interactions

| Scenario | Behavior |
|----------|----------|
| Zoomed pane (`Ctrl+b z`) | Full frame border (single-pane appearance), title shows `"[Z] 0: bash"` |
| Copy mode | Title bar prefix: `"[CPY] 0: bash"` |
| `pane-border-status off` | No title bar, no single-pane frame. Reverts to current behavior. Status bar enhancement still active. |
| Popup panes (`display-popup`) | No frame border — popups have their own bordered widget |
| Status bar + unfocused | `status-unfocused-style` applies independently of pane border features |

## Config Defaults Summary

| Option | Default | tmux-compatible |
|--------|---------|-----------------|
| `pane-border-status` | `top` | Yes (tmux default is `off`) |
| `pane-border-format` | `"#{pane_index}: #{pane_title}"` | Yes |
| `status-unfocused-style` | `""` (auto-desaturate) | No (psmux extension) |
| `pane-border-style` | `""` | Yes (existing) |
| `pane-active-border-style` | `"fg=green"` | Yes (existing) |
| `pane-border-unfocused-style` | `"fg=darkgray,dim"` | No (existing psmux extension) |

Note: psmux defaults `pane-border-status` to `top` (tmux defaults to `off`) because the whole point of this feature is visibility by default.

## Testing Plan

| Test | Verification |
|------|-------------|
| Single pane | Frame border visible, title shows `"0: <title>"`, PTY resized to (w-2, h-2) |
| Two panes (horizontal) | Each has title bar, separator between, PTY at (w, h-1) |
| Two panes (vertical) | Each has title bar, separator between, PTY at (w, h-1) |
| Three+ panes (mixed) | Title bars on all, separators correct, intersections fixed |
| Zoomed pane | Frame border like single pane, `[Z]` prefix in title |
| `pane-border-status off` | No frame, no title, identical to pre-feature behavior |
| `pane-border-status bottom` | Title bar at bottom of each pane instead of top |
| Alt-tab away | Status bar desaturates/changes, all borders go unfocused style |
| `set -g status-unfocused-style "bg=red"` | Explicit override applies on unfocus |
| Popup pane | No frame border on popup |
| Copy mode | `[CPY]` prefix in title bar |
| `pane-border-format` custom | Custom format string renders correctly |

## Files Changed

| File | Changes |
|------|---------|
| `src/rendering.rs` | Title bar rendering in `render_node()` Leaf case, single-pane frame border, corner chars in `fix_border_intersections()` |
| `src/app.rs` | Status bar unfocused style logic |
| `src/types.rs` | New AppState fields: `pane_border_status`, `pane_border_format`, `status_unfocused_style` |
| `src/config.rs` | Parse new options, promote from `user_options` |
| `src/server/options.rs` | Register get/set for new options |
| `src/style.rs` | `desaturate_style()` helper |
| `src/format.rs` | Ensure pane-context format expansion for title bar |
