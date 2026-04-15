# Pane Focus Visibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the active pane unmistakably visible via per-pane title bars, single-pane frame borders, and enhanced status bar focus styling.

**Architecture:** Three features share a unified frame model. Every pane reserves 1 row for a title bar (configurable `pane-border-status top|bottom|off`). Single-pane windows additionally draw a full box border (left/right/bottom edges). The status bar auto-desaturates on window unfocus, with an optional `status-unfocused-style` override.

**Tech Stack:** Rust, ratatui (TUI framework), crossterm (terminal backend), vt100-psmux (terminal parser)

**Spec:** `docs/superpowers/specs/2026-03-29-pane-focus-visibility-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src/types.rs` | Modify | Add `pane_border_status`, `pane_border_format`, `status_unfocused_style` to AppState |
| `src/config.rs` | Modify | Parse new options from config file |
| `src/server/options.rs` | Modify | Register get/set for new options |
| `src/style.rs` | Modify | Add `desaturate_color()` and `desaturate_style()` helpers |
| `src/rendering.rs` | Modify | Title bar rendering, single-pane frame border, pass pane context |
| `src/app.rs` | Modify | Status bar unfocused style logic |
| `src/format.rs` | Read-only | Already supports pane-context expansion via `PANE_POS_OVERRIDE` |

---

### Task 1: Add AppState Fields and Config Parsing

**Files:**
- Modify: `src/types.rs:598-603` (AppState fields) and `src/types.rs:825-830` (defaults)
- Modify: `src/config.rs:652-660` (option parsing)
- Modify: `src/server/options.rs:154-156` (get) and `src/server/options.rs:478-483` (set)

- [ ] **Step 1: Add fields to AppState**

In `src/types.rs`, after the `pane_border_unfocused_style` field (~line 603), add:

```rust
    /// pane-border-status: "top", "bottom", or "off" (default "top")
    pub pane_border_status: String,
    /// pane-border-format: format string for pane title bars
    pub pane_border_format: String,
    /// status-unfocused-style: explicit style override when window loses focus (empty = auto-desaturate)
    pub status_unfocused_style: String,
```

In the `Default` impl (~line 830), after `pane_border_unfocused_style`, add:

```rust
            pane_border_status: "top".to_string(),
            pane_border_format: "#{pane_index}: #{pane_title}".to_string(),
            status_unfocused_style: String::new(),
```

- [ ] **Step 2: Add config parsing**

In `src/config.rs`, in the `apply_option()` function, after the `"pane-border-unfocused-style"` arm (~line 660), add:

```rust
        "pane-border-status" => {
            match value {
                "top" | "bottom" | "off" => app.pane_border_status = value.to_string(),
                _ => {} // ignore invalid values
            }
        }
        "pane-border-format" => {
            app.pane_border_format = value.to_string();
        }
        "status-unfocused-style" => {
            app.status_unfocused_style = value.to_string();
        }
```

Also remove the existing `user_options` passthrough for `"pane-border-status"` and `"pane-border-format"` in the `"window-style" | "window-active-style"` arm (~line 746) — these are now first-class options. Find:

```rust
        "window-style" | "window-active-style" => {
            app.user_options.insert(key.to_string(), value.to_string());
        }
```

Change to:

```rust
        "window-style" | "window-active-style" => {
            app.user_options.insert(key.to_string(), value.to_string());
        }
```

And the separate `"pane-border-format" | "pane-border-status"` arm that currently stores in `user_options` — remove it entirely since we now handle them above.

- [ ] **Step 3: Register get/set in options.rs**

In `src/server/options.rs`, in `get_option_value()` after `"pane-active-border-style"` (~line 156), add:

```rust
        "pane-border-unfocused-style" => app.pane_border_unfocused_style.clone(),
        "pane-border-status" => app.pane_border_status.clone(),
        "pane-border-format" => app.pane_border_format.clone(),
        "status-unfocused-style" => app.status_unfocused_style.clone(),
```

In the `set_option()` function after `"pane-active-border-style"` (~line 483), add:

```rust
        "pane-border-status" => {
            match value {
                "top" | "bottom" | "off" => app.pane_border_status = value.to_string(),
                _ => {}
            }
        }
        "pane-border-format" => {
            app.pane_border_format = value.to_string();
        }
        "status-unfocused-style" => {
            app.status_unfocused_style = value.to_string();
        }
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`
Expected: Compiles with no errors (may have warnings about unused fields — that's fine, we'll use them in later tasks).

- [ ] **Step 5: Commit**

```bash
git add src/types.rs src/config.rs src/server/options.rs
git commit -m "feat(config): add pane-border-status, pane-border-format, status-unfocused-style options"
```

---

### Task 2: Add desaturate_style() Helper

**Files:**
- Modify: `src/style.rs` (add helper functions)

- [ ] **Step 1: Write desaturate_color() and desaturate_style()**

In `src/style.rs`, after the existing `parse_tmux_style()` function (at the end of the file), add:

```rust
/// Convert a ratatui Color to its grayscale equivalent using luminance weighting.
/// Formula: gray = 0.299*R + 0.587*G + 0.114*B
pub fn desaturate_color(c: Color) -> Color {
    match c {
        Color::Rgb(r, g, b) => {
            let gray = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) as u8;
            Color::Rgb(gray, gray, gray)
        }
        // Map named colors to approximate grayscale RGB
        Color::Red => Color::Rgb(76, 76, 76),
        Color::Green => Color::Rgb(75, 75, 75),
        Color::Blue => Color::Rgb(29, 29, 29),
        Color::Yellow => Color::Rgb(150, 150, 150),
        Color::Magenta => Color::Rgb(53, 53, 53),
        Color::Cyan => Color::Rgb(117, 117, 117),
        Color::White => Color::Rgb(200, 200, 200),
        Color::Black => Color::Rgb(20, 20, 20),
        Color::Gray => Color::Rgb(128, 128, 128),
        Color::DarkGray => Color::Rgb(80, 80, 80),
        Color::LightRed => Color::Rgb(120, 120, 120),
        Color::LightGreen => Color::Rgb(120, 120, 120),
        Color::LightBlue => Color::Rgb(80, 80, 80),
        Color::LightYellow => Color::Rgb(180, 180, 180),
        Color::LightMagenta => Color::Rgb(100, 100, 100),
        Color::LightCyan => Color::Rgb(150, 150, 150),
        Color::Indexed(i) => {
            // For 256-color palette, just use a mid-gray
            if i < 8 {
                Color::Rgb(80, 80, 80)
            } else if i < 16 {
                Color::Rgb(120, 120, 120)
            } else {
                Color::Rgb(100, 100, 100)
            }
        }
        Color::Reset => Color::Reset,
    }
}

/// Desaturate a full Style — convert fg/bg to grayscale and add DIM modifier.
pub fn desaturate_style(style: Style) -> Style {
    let mut result = style;
    if let Some(fg) = style.fg {
        result.fg = Some(desaturate_color(fg));
    }
    if let Some(bg) = style.bg {
        result.bg = Some(desaturate_color(bg));
    }
    result = result.add_modifier(Modifier::DIM);
    result
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles (functions unused for now).

- [ ] **Step 3: Commit**

```bash
git add src/style.rs
git commit -m "feat(style): add desaturate_color() and desaturate_style() helpers"
```

---

### Task 3: Enhanced Status Bar Focus Styling

**Files:**
- Modify: `src/app.rs:685-689` (status bar unfocus logic)

- [ ] **Step 1: Import desaturate_style**

In `src/app.rs`, find the existing import from `crate::style`:

```rust
use crate::style::parse_tmux_style;
```

Add `desaturate_style`:

```rust
use crate::style::{desaturate_style, parse_tmux_style};
```

- [ ] **Step 2: Replace the DIM-only unfocused logic**

In `src/app.rs`, find (~line 685):

```rust
            let final_status_style = if app.window_focused {
                base_status_style
            } else {
                base_status_style.add_modifier(Modifier::DIM)
            };
```

Replace with:

```rust
            let final_status_style = if app.window_focused {
                base_status_style
            } else if !app.status_unfocused_style.is_empty() {
                parse_tmux_style(&app.status_unfocused_style)
            } else {
                desaturate_style(base_status_style)
            };
```

- [ ] **Step 3: Verify it compiles and run**

Run: `cargo check`
Expected: Compiles.

Manual test: Start psmux, alt-tab away. Status bar should visibly desaturate (grayscale + dim) instead of just dimming. Alt-tab back — normal colors restore.

- [ ] **Step 4: Test explicit override**

Run: `psmux set -g status-unfocused-style "bg=red,fg=white"`

Alt-tab away — status bar should turn red. Alt-tab back — green again.

Run: `psmux set -g status-unfocused-style ""`

Alt-tab away — back to auto-desaturation.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(status): auto-desaturate status bar on window unfocus + status-unfocused-style override"
```

---

### Task 4: Per-Pane Title Bar Rendering (Multi-Pane)

This is the core feature. Modify `render_node()` to reserve a title bar row for each pane.

**Files:**
- Modify: `src/rendering.rs:140-170` (render_window — pass new params)
- Modify: `src/rendering.rs:241-370` (render_node — Leaf case title bar)

- [ ] **Step 1: Add pane_border_status and pane_border_format params to render_node**

In `src/rendering.rs`, modify the `render_node` signature to add three new parameters. Find:

```rust
#[allow(clippy::too_many_arguments)]
pub fn render_node(
    f: &mut Frame,
    node: &mut Node,
    active_path: &Vec<usize>,
    cur_path: &mut Vec<usize>,
    area: Rect,
    dim_preds: bool,
    border_style: Style,
    active_border_style: Style,
    unfocused_border_style: Style,
    window_focused: bool,
    copy_cursor: Option<(u16, u16)>,
    active_rect: Option<Rect>,
    zoomed: bool,
) {
```

Replace with:

```rust
#[allow(clippy::too_many_arguments)]
pub fn render_node(
    f: &mut Frame,
    node: &mut Node,
    active_path: &Vec<usize>,
    cur_path: &mut Vec<usize>,
    area: Rect,
    dim_preds: bool,
    border_style: Style,
    active_border_style: Style,
    unfocused_border_style: Style,
    window_focused: bool,
    copy_cursor: Option<(u16, u16)>,
    active_rect: Option<Rect>,
    zoomed: bool,
    pane_border_status: &str,
    pane_border_format: &str,
    is_root: bool,
    app: &AppState,
) {
```

- [ ] **Step 2: Update render_window to pass new params**

In `render_window()`, find the `render_node(` call (~line 156). Add the new arguments at the end:

```rust
    render_node(
        f,
        &mut win.root,
        &win.active_path,
        &mut Vec::new(),
        area,
        dim_preds,
        border_style,
        active_border_style,
        unfocused_border_style,
        app.window_focused,
        copy_cursor,
        active_rect,
        zoomed,
        &app.pane_border_status,
        &app.pane_border_format,
        true,
        app,
    );
```

Note: This requires borrowing `app` after the mutable borrow of `win`. To fix the borrow issue, extract the needed values before the mutable borrow. At the top of `render_window`, before `let win = &mut app.windows[app.active_idx]`:

```rust
    let pane_border_status = app.pane_border_status.clone();
    let pane_border_format = app.pane_border_format.clone();
```

Then pass `&pane_border_status` and `&pane_border_format` instead of `&app.pane_border_status` and `&app.pane_border_format`. And pass a raw pointer or skip the `app` param — instead, expand the format string *before* calling render_node. Actually, the simplest approach: we need pane-specific format expansion at render time. Since format expansion needs `&AppState` and a pane position, we should pre-expand in render_node's Leaf case by calling `set_pane_pos_override` + `expand_format`. But `expand_format` needs `&AppState`, and render_node already has a mutable borrow on the window's root node.

**Revised approach**: Don't pass `app` to `render_node`. Instead, pre-compute the title text for each pane in `render_window` and pass it via a HashMap.

Before the `render_node` call in `render_window`, build a map of pane titles:

```rust
    let pane_border_status = app.pane_border_status.clone();
    let pane_border_format = app.pane_border_format.clone();

    // Pre-expand title bar text for each pane
    let mut pane_titles: std::collections::HashMap<usize, String> = std::collections::HashMap::new();
    if pane_border_status != "off" {
        let pane_ids = crate::tree::collect_all_pane_ids(&app.windows[app.active_idx].root);
        for (pos, pane_id) in pane_ids.iter().enumerate() {
            crate::format::set_pane_pos_override(Some(pos));
            let title = crate::format::expand_format(&pane_border_format, app);
            pane_titles.insert(*pane_id, title);
            crate::format::set_pane_pos_override(None);
        }
    }
```

Then change the `render_node` signature to take `&HashMap<usize, String>` instead of `app: &AppState`:

```rust
    pane_border_status: &str,
    pane_titles: &std::collections::HashMap<usize, String>,
    is_root: bool,
```

This avoids the borrow conflict entirely.

- [ ] **Step 3: Add collect_all_pane_ids helper to tree.rs**

In `src/tree.rs`, add:

```rust
/// Collect all pane IDs in tree order (left-to-right, top-to-bottom).
pub fn collect_all_pane_ids(node: &Node) -> Vec<usize> {
    let mut ids = Vec::new();
    collect_pane_ids_inner(node, &mut ids);
    ids
}

fn collect_pane_ids_inner(node: &Node, ids: &mut Vec<usize>) {
    match node {
        Node::Leaf(pane) => ids.push(pane.id),
        Node::Split { children, .. } => {
            for child in children {
                collect_pane_ids_inner(child, ids);
            }
        }
    }
}
```

Also add a public setter for `PANE_POS_OVERRIDE` in `src/format.rs` if not already public. Check — the thread-local `PANE_POS_OVERRIDE` is set via `PANE_POS_OVERRIDE.set()` directly in format.rs. We need a public function. Add to `src/format.rs`:

```rust
/// Set the pane position override for per-pane format expansion.
pub fn set_pane_pos_override(pos: Option<usize>) {
    PANE_POS_OVERRIDE.set(pos);
}
```

- [ ] **Step 4: Implement title bar rendering in Leaf case**

In `render_node()`, at the start of the `Node::Leaf(pane)` arm, after `let is_active = *cur_path == *active_path;`, add title bar logic:

```rust
        Node::Leaf(pane) => {
            let is_active = *cur_path == *active_path;

            // ── Title bar ──
            let has_title_bar = pane_border_status != "off" && area.height >= 3;
            let is_single_pane = is_root && matches!(node, Node::Leaf(_));

            let (title_area, content_area) = if has_title_bar {
                if is_single_pane {
                    // Full box: top title row, left/right cols, bottom row
                    let title_rect = Rect::new(area.x, area.y, area.width, 1);
                    let inner = Rect::new(
                        area.x + 1,
                        area.y + 1,
                        area.width.saturating_sub(2),
                        area.height.saturating_sub(2),
                    );
                    (Some(title_rect), inner)
                } else if pane_border_status == "bottom" {
                    let content = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
                    let title_rect = Rect::new(area.x, area.y + content.height, area.width, 1);
                    (Some(title_rect), content)
                } else {
                    // top (default)
                    let title_rect = Rect::new(area.x, area.y, area.width, 1);
                    let content = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(1));
                    (Some(title_rect), content)
                }
            } else {
                (None, area)
            };

            // Render title bar
            if let Some(title_rect) = title_area {
                let title_style = if !window_focused {
                    unfocused_border_style
                } else if is_active {
                    active_border_style
                } else {
                    border_style
                };

                let title_text = pane_titles
                    .get(&pane.id)
                    .map(|s| s.as_str())
                    .unwrap_or("");

                let buf = f.buffer_mut();
                if is_single_pane {
                    // Full box: ┌─ title ──...──┐
                    let w = title_rect.width as usize;
                    if w >= 4 {
                        let y = title_rect.y;
                        let x0 = title_rect.x;
                        // Top-left corner
                        let idx = (y - buf.area.y) as usize * buf.area.width as usize
                            + (x0 - buf.area.x) as usize;
                        if idx < buf.content.len() {
                            buf.content[idx].set_char('┌');
                            buf.content[idx].set_style(title_style);
                        }
                        // "─ title " then fill with ─
                        let title_display = if title_text.is_empty() {
                            String::new()
                        } else {
                            format!(" {} ", title_text)
                        };
                        let title_chars: Vec<char> = title_display.chars().collect();
                        for col in 1..w.saturating_sub(1) {
                            let ci = col - 1;
                            let ch = if ci < title_chars.len() {
                                title_chars[ci]
                            } else {
                                '─'
                            };
                            let idx = (y - buf.area.y) as usize * buf.area.width as usize
                                + (x0 + col as u16 - buf.area.x) as usize;
                            if idx < buf.content.len() {
                                buf.content[idx].set_char(ch);
                                buf.content[idx].set_style(title_style);
                            }
                        }
                        // Top-right corner
                        let idx = (y - buf.area.y) as usize * buf.area.width as usize
                            + (x0 + w as u16 - 1 - buf.area.x) as usize;
                        if idx < buf.content.len() {
                            buf.content[idx].set_char('┐');
                            buf.content[idx].set_style(title_style);
                        }
                    }
                } else {
                    // Multi-pane: ── title ──...──
                    let w = title_rect.width as usize;
                    let y = title_rect.y;
                    let x0 = title_rect.x;
                    let title_display = if title_text.is_empty() {
                        String::new()
                    } else {
                        format!(" {} ", title_text)
                    };
                    let title_chars: Vec<char> = title_display.chars().collect();
                    for col in 0..w {
                        let ch = if col < title_chars.len() {
                            title_chars[col]
                        } else {
                            '─'
                        };
                        let idx = (y - buf.area.y) as usize * buf.area.width as usize
                            + (x0 + col as u16 - buf.area.x) as usize;
                        if idx < buf.content.len() {
                            buf.content[idx].set_char(ch);
                            buf.content[idx].set_style(title_style);
                        }
                    }
                }
            }

            let inner = content_area;
            // ... rest of existing Leaf rendering (PTY content) uses `inner` ...
```

Replace the existing `let inner = area;` line with the computed `content_area` above.

- [ ] **Step 5: Render single-pane left/right/bottom borders**

After the title bar rendering block, still inside the `has_title_bar && is_single_pane` path, add side and bottom borders:

```rust
            // Single-pane: draw left, right, and bottom borders
            if has_title_bar && is_single_pane {
                let title_style = if !window_focused {
                    unfocused_border_style
                } else if is_active {
                    active_border_style
                } else {
                    border_style
                };
                let buf = f.buffer_mut();
                // Left border: │ from row area.y+1 to area.y+area.height-2
                for y in (area.y + 1)..(area.y + area.height.saturating_sub(1)) {
                    let idx = (y - buf.area.y) as usize * buf.area.width as usize
                        + (area.x - buf.area.x) as usize;
                    if idx < buf.content.len() {
                        buf.content[idx].set_char('│');
                        buf.content[idx].set_style(title_style);
                    }
                }
                // Right border: │
                let right_x = area.x + area.width.saturating_sub(1);
                for y in (area.y + 1)..(area.y + area.height.saturating_sub(1)) {
                    let idx = (y - buf.area.y) as usize * buf.area.width as usize
                        + (right_x - buf.area.x) as usize;
                    if idx < buf.content.len() {
                        buf.content[idx].set_char('│');
                        buf.content[idx].set_style(title_style);
                    }
                }
                // Bottom border: └──...──┘
                let bottom_y = area.y + area.height.saturating_sub(1);
                let w = area.width as usize;
                if w >= 2 {
                    // Bottom-left corner
                    let idx = (bottom_y - buf.area.y) as usize * buf.area.width as usize
                        + (area.x - buf.area.x) as usize;
                    if idx < buf.content.len() {
                        buf.content[idx].set_char('└');
                        buf.content[idx].set_style(title_style);
                    }
                    // Bottom line
                    for col in 1..w.saturating_sub(1) {
                        let idx = (bottom_y - buf.area.y) as usize * buf.area.width as usize
                            + (area.x + col as u16 - buf.area.x) as usize;
                        if idx < buf.content.len() {
                            buf.content[idx].set_char('─');
                            buf.content[idx].set_style(title_style);
                        }
                    }
                    // Bottom-right corner
                    let idx = (bottom_y - buf.area.y) as usize * buf.area.width as usize
                        + (area.x + w as u16 - 1 - buf.area.x) as usize;
                    if idx < buf.content.len() {
                        buf.content[idx].set_char('┘');
                        buf.content[idx].set_style(title_style);
                    }
                }
            }
```

This goes *after* the PTY content rendering (after the `f.render_widget(para, inner);` call) so the border draws on top.

- [ ] **Step 6: Update the recursive call in Split case**

In the `Node::Split` arm, find the recursive `render_node(` call and add the new parameters:

```rust
                    render_node(
                        f,
                        child,
                        active_path,
                        cur_path,
                        rects[i],
                        dim_preds,
                        border_style,
                        active_border_style,
                        unfocused_border_style,
                        window_focused,
                        copy_cursor,
                        active_rect,
                        zoomed,
                        pane_border_status,
                        pane_titles,
                        false, // not root — children of a split are never the root
                    );
```

- [ ] **Step 7: Verify it compiles**

Run: `cargo check`
Expected: Compiles. Fix any borrow issues (the pre-computed pane_titles approach should avoid them).

- [ ] **Step 8: Commit**

```bash
git add src/rendering.rs src/tree.rs src/format.rs
git commit -m "feat(rendering): per-pane title bars and single-pane frame borders"
```

---

### Task 5: Handle pane-border-status bottom for Single Pane

**Files:**
- Modify: `src/rendering.rs` (single-pane bottom variant)

- [ ] **Step 1: Adjust single-pane layout for bottom mode**

In the `has_title_bar` layout computation (Task 4 Step 4), the `is_single_pane` branch currently always puts the title at the top. Add bottom support:

```rust
                if is_single_pane {
                    if pane_border_status == "bottom" {
                        // Box with title on bottom: ┌───┐ ... └─ title ─┘
                        let top_rect = Rect::new(area.x, area.y, area.width, 1);
                        let inner = Rect::new(
                            area.x + 1,
                            area.y + 1,
                            area.width.saturating_sub(2),
                            area.height.saturating_sub(2),
                        );
                        // title_area is bottom row, but we render it as the bottom border with text
                        (Some(top_rect), inner)
                    } else {
                        // top (default): ┌─ title ─┐ ... └───┘
                        let title_rect = Rect::new(area.x, area.y, area.width, 1);
                        let inner = Rect::new(
                            area.x + 1,
                            area.y + 1,
                            area.width.saturating_sub(2),
                            area.height.saturating_sub(2),
                        );
                        (Some(title_rect), inner)
                    }
                }
```

For the bottom case, the title text goes in the bottom border row instead of the top. Adjust the bottom border rendering in Step 5 to embed the title text when `pane_border_status == "bottom"`:

```rust
                    // Bottom border: └─ title ──┘ (if bottom mode) or └──...──┘
                    let bottom_title = if pane_border_status == "bottom" {
                        pane_titles.get(&pane.id).map(|s| s.as_str()).unwrap_or("")
                    } else {
                        ""
                    };
                    let bottom_display = if bottom_title.is_empty() {
                        String::new()
                    } else {
                        format!(" {} ", bottom_title)
                    };
                    let bottom_chars: Vec<char> = bottom_display.chars().collect();
                    // ... render └ then bottom_chars/─ fill then ┘
```

And for the top border in bottom mode, render a plain `┌──...──┐` without title text.

- [ ] **Step 2: Verify and commit**

Run: `cargo check`

```bash
git add src/rendering.rs
git commit -m "feat(rendering): support pane-border-status bottom for single-pane frame"
```

---

### Task 6: Handle Zoomed Pane and Mode Prefixes

**Files:**
- Modify: `src/rendering.rs` (title bar content adjustments)

- [ ] **Step 1: Add mode prefix to title text**

In `render_window()`, when building the `pane_titles` map, prepend mode indicator. After expanding the format:

```rust
        for (pos, pane_id) in pane_ids.iter().enumerate() {
            crate::format::set_pane_pos_override(Some(pos));
            let mut title = crate::format::expand_format(&pane_border_format, app);
            crate::format::set_pane_pos_override(None);

            // Prepend mode prefix for the active pane
            let active_id = crate::tree::get_active_pane_id(
                &app.windows[app.active_idx].root,
                &app.windows[app.active_idx].active_path,
            );
            if Some(*pane_id) == active_id {
                let prefix = match app.mode {
                    Mode::CopyMode => "[CPY] ",
                    Mode::CopySearch { .. } => "[SEARCH] ",
                    _ => "",
                };
                if !prefix.is_empty() {
                    title = format!("{}{}", prefix, title);
                }
                if zoomed {
                    title = format!("[Z] {}", title);
                }
            }

            pane_titles.insert(*pane_id, title);
        }
```

- [ ] **Step 2: Zoomed pane uses single-pane frame**

In `render_node`, the `is_single_pane` detection should also trigger when zoomed:

```rust
            let is_single_pane = is_root && (matches!(node, Node::Leaf(_)) || zoomed);
```

Wait — when zoomed, `render_node` is called on the Leaf pane directly (because the zoom logic in `render_window` already selects the zoomed pane). Actually, let me check. The zoom logic is in `render_window` — if zoomed, only the active pane is rendered. The `is_root` + `Node::Leaf` check already handles this since the zoomed pane IS a leaf at the root call.

Confirm: when `zoomed == true`, the existing border-drawing code in the Split arm does `if zoomed { return; }`. The Leaf arm at root level with `is_root=true` will correctly draw the single-pane frame. No change needed here.

- [ ] **Step 3: Verify and commit**

Run: `cargo check`

```bash
git add src/rendering.rs
git commit -m "feat(rendering): mode prefixes and zoom indicator in pane title bars"
```

---

### Task 7: Skip Frame for Popup Panes

**Files:**
- Modify: `src/rendering.rs` (popup detection)

- [ ] **Step 1: Check popup rendering path**

Popup panes are rendered via `display-popup` which uses a separate ratatui `Block` widget — they don't go through `render_node`. Verify this by checking `src/popup.rs` or the popup rendering in `src/app.rs`.

If popups are rendered through `render_node` (unlikely), add a `is_popup: bool` parameter. If not, no change needed — the frame border only applies to `render_node` callers.

- [ ] **Step 2: Verify no-op and commit if needed**

Run: `cargo test`

If popups are already separate, this task is a no-op verification.

---

### Task 8: Wire Up Server-Side Option Reset Defaults

**Files:**
- Modify: `src/server/mod.rs` (option reset handling)

- [ ] **Step 1: Add reset defaults for new options**

Search for the existing `pane-active-border-style` reset in `src/server/mod.rs` (the `set -gu` / unset path). Find where option defaults are restored and add:

```rust
                                    "pane-border-status" => {
                                        app.pane_border_status = "top".to_string();
                                    }
                                    "pane-border-format" => {
                                        app.pane_border_format = "#{pane_index}: #{pane_title}".to_string();
                                    }
                                    "status-unfocused-style" => {
                                        app.status_unfocused_style = String::new();
                                    }
```

- [ ] **Step 2: Add to show-options dump**

Search for where `pane-active-border-style` is dumped in `show-options` output and add adjacent entries for the new options.

- [ ] **Step 3: Add to JSON state serialization**

Search for where `pane_active_border_style` is serialized in the server's JSON frame output (the large `format_args!` call). Add the new fields so they're available to the client renderer.

- [ ] **Step 4: Verify and commit**

Run: `cargo check && cargo test`

```bash
git add src/server/mod.rs
git commit -m "feat(server): wire pane-border-status/format and status-unfocused-style into option reset, show, and JSON"
```

---

### Task 9: Integration Testing

**Files:**
- Modify: `tests/` (add new integration tests)

- [ ] **Step 1: Test config parsing**

Run psmux and verify options are accepted:

```bash
psmux new-session -d -s test
psmux set -g pane-border-status top
psmux show-options -g pane-border-status
# Expected: pane-border-status top

psmux set -g pane-border-format "#{pane_index}:#{pane_title}"
psmux show-options -g pane-border-format
# Expected: pane-border-format #{pane_index}:#{pane_title}

psmux set -g status-unfocused-style "bg=red"
psmux show-options -g status-unfocused-style
# Expected: status-unfocused-style bg=red

psmux set -g pane-border-status off
psmux show-options -g pane-border-status
# Expected: pane-border-status off
```

- [ ] **Step 2: Run full test suite**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: All pass.

- [ ] **Step 3: Manual visual testing**

1. Start psmux with single pane — verify frame border with title
2. Split horizontally (`Ctrl+b %`) — verify both panes have title bars
3. Split vertically (`Ctrl+b "`) — verify title bars on all panes
4. Zoom a pane (`Ctrl+b z`) — verify frame border appears
5. Enter copy mode (`Ctrl+b [`) — verify `[CPY]` prefix
6. Alt-tab away — verify status bar desaturates and borders go unfocused style
7. `set -g pane-border-status off` — verify titles disappear, no frame
8. `set -g pane-border-status bottom` — verify title moves to bottom

- [ ] **Step 4: Commit any fixes**

```bash
git add -A
git commit -m "test: integration verification for pane focus visibility features"
```

---

### Task 10: Update Feature Registry

**Files:**
- Modify: `docs/ohboy-builds-features.md`

- [ ] **Step 1: Add Pane Focus Visibility to registry**

Add after the Enhanced Format Engine entry:

```markdown
### 11. Pane Focus Visibility (Frame + Title Bar + Status Desaturation)
- **Files**: `src/rendering.rs` (title bar, frame border), `src/app.rs` (status desaturation), `src/style.rs` (desaturate helpers)
- **Config**: `pane-border-status`, `pane-border-format`, `status-unfocused-style`
- **Test**: `cargo test` + manual visual check
- **Verify**: Start psmux, verify title bar visible, split panes to check multi-pane titles, alt-tab to check status desaturation
- **Risk from upstream**: Changes to `render_node` signature, `src/rendering.rs` border drawing, `src/app.rs` status bar rendering
```

- [ ] **Step 2: Commit**

```bash
git add docs/ohboy-builds-features.md
git commit -m "docs: add pane focus visibility to ohboy-builds feature registry"
```
