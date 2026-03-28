# Three-State Pane Focus Borders Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Visually distinguish pane borders based on OS-level window focus — active pane is bright when psmux is focused, all borders dim when psmux is backgrounded.

**Architecture:** Add a `window_focused: bool` to `AppState`, toggled by existing `FocusIn`/`FocusOut` handlers. Rendering uses a three-way style lookup: unfocused overrides everything, then active vs inactive. A new `pane-border-unfocused-style` config option controls the dimmed appearance.

**Tech Stack:** Rust, ratatui (Style/Modifier), crossterm (focus events), existing `parse_tmux_style()` parser.

---

## File Map

| File | Change |
|---|---|
| `src/types.rs` | Add `window_focused: bool` + `pane_border_unfocused_style: String` fields |
| `src/config.rs` | Parse `pane-border-unfocused-style` set-option |
| `src/format.rs` | Display value for `show-options` |
| `src/rendering.rs` | Accept + use unfocused style in border drawing |
| `src/app.rs` | Pass unfocused style to render_window, dim status bar |
| `src/server/mod.rs` | Toggle `window_focused` on FocusIn/FocusOut, trigger redraw |
| `tests/focus_border_tests.rs` | Unit tests for three-way style selection |

---

### Task 1: Add `window_focused` and `pane_border_unfocused_style` to AppState

**Files:**
- Modify: `src/types.rs:549` (after `focus_events` field)
- Modify: `src/types.rs:598` (after `pane_active_border_style` field)
- Modify: `src/types.rs:720` (in `AppState::new()` initializer)

- [ ] **Step 1: Add `window_focused` field to AppState struct**

In `src/types.rs`, after line 549 (`pub focus_events: bool`), add:

```rust
    /// Whether the terminal window currently has OS-level focus.
    /// Used to dim pane borders and status bar when psmux is backgrounded.
    pub window_focused: bool,
```

- [ ] **Step 2: Add `pane_border_unfocused_style` field to AppState struct**

In `src/types.rs`, after line 598 (`pub pane_active_border_style: String`), add:

```rust
    /// pane-border-unfocused-style: style for all pane borders when window lacks OS focus
    pub pane_border_unfocused_style: String,
```

- [ ] **Step 3: Initialize both fields in `AppState::new()`**

In `src/types.rs`, in the `Self { ... }` block of `AppState::new()`:

After the `focus_events: false,` line, add:
```rust
            window_focused: true,
```

After the `pane_active_border_style: "fg=green".to_string(),` line, add:
```rust
            pane_border_unfocused_style: "fg=darkgray,dim".to_string(),
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check 2>&1 | head -5`
Expected: warnings only (unused field), no errors

- [ ] **Step 5: Commit**

```bash
git add src/types.rs
git commit -m "feat: add window_focused and pane_border_unfocused_style to AppState"
```

---

### Task 2: Config parsing and show-options support

**Files:**
- Modify: `src/config.rs:652-657` (alongside existing border style parsing)
- Modify: `src/format.rs:1003-1004` (alongside existing border style display)

- [ ] **Step 1: Add config parsing in `src/config.rs`**

Find the block:
```rust
        "pane-active-border-style" => {
            app.pane_active_border_style = value.to_string();
        }
```

Immediately after it, add:
```rust
        "pane-border-unfocused-style" => {
            app.pane_border_unfocused_style = value.to_string();
        }
```

- [ ] **Step 2: Add show-options display in `src/format.rs`**

Find the block:
```rust
        "pane-active-border-style" => Some(app.pane_active_border_style.clone()),
```

Immediately after it, add:
```rust
        "pane-border-unfocused-style" => Some(app.pane_border_unfocused_style.clone()),
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | head -5`
Expected: clean or warnings only

- [ ] **Step 4: Commit**

```bash
git add src/config.rs src/format.rs
git commit -m "feat: parse and display pane-border-unfocused-style option"
```

---

### Task 3: Toggle `window_focused` on FocusIn/FocusOut

**Files:**
- Modify: `src/server/mod.rs:4614-4654` (FocusIn/FocusOut handlers)

- [ ] **Step 1: Set `window_focused = true` on FocusIn**

In `src/server/mod.rs`, find the `CtrlReq::FocusIn` handler (line 4614). Add these two lines at the very start of the block, before the `if app.focus_events` check:

```rust
                        CtrlReq::FocusIn => {
                            app.window_focused = true;
                            state_dirty = true;
                            if app.focus_events {
```

The `state_dirty = true` triggers a re-render so borders update immediately.

- [ ] **Step 2: Set `window_focused = false` on FocusOut**

In the `CtrlReq::FocusOut` handler (line 4635), add the same pattern:

```rust
                        CtrlReq::FocusOut => {
                            app.window_focused = false;
                            state_dirty = true;
                            if app.focus_events {
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | head -5`
Expected: clean

- [ ] **Step 4: Commit**

```bash
git add src/server/mod.rs
git commit -m "feat: toggle window_focused on FocusIn/FocusOut events"
```

---

### Task 4: Three-way border style selection in rendering

**Files:**
- Modify: `src/rendering.rs:119-147` (render_window function)
- Modify: `src/rendering.rs:217-229` (render_node signature)
- Modify: `src/rendering.rs:382-484` (separator drawing logic)

- [ ] **Step 1: Pass unfocused style from `render_window`**

In `src/rendering.rs`, update `render_window` (line 119). After the existing style parsing (lines 121-122):

```rust
    let border_style = parse_tmux_style(&app.pane_border_style);
    let active_border_style = parse_tmux_style(&app.pane_active_border_style);
```

Add:
```rust
    let unfocused_border_style = parse_tmux_style(&app.pane_border_unfocused_style);
```

Then update the `render_node` call (line 134) to pass it. Add `app.window_focused` and `unfocused_border_style` as additional arguments:

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
    );
```

- [ ] **Step 2: Update `render_node` signature**

Update the `render_node` function signature (line 217) to accept the two new parameters. Insert them after `active_border_style`:

```rust
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

- [ ] **Step 3: Pass new params through the recursive call**

In the `Node::Split` branch, find the recursive `render_node` call (around line 350-365). Add the two new arguments in the same positions:

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
                    );
```

- [ ] **Step 4: Apply three-way logic to separator drawing**

In the separator drawing section (lines 382-484), every place that currently resolves a style as either `active_border_style` or `border_style` needs to first check `window_focused`. The simplest approach: compute the effective styles once at the top of the separator block.

Right after `let buf = f.buffer_mut();` (line 374), add:

```rust
            // Three-way style: unfocused overrides everything
            let eff_border = if window_focused { border_style } else { unfocused_border_style };
            let eff_active = if window_focused { active_border_style } else { unfocused_border_style };
```

Then replace all occurrences of `border_style` in the separator section (lines 382-484) with `eff_border`, and all occurrences of `active_border_style` with `eff_active`. Specifically:

- Line 393-396: `active_border_style` → `eff_active`, `border_style` → `eff_border`
- Line 398-401: `active_border_style` → `eff_active`, `border_style` → `eff_border`
- Line 419-421: `active_border_style` → `eff_active`, `border_style` → `eff_border`
- Line 443-447: `active_border_style` → `eff_active`, `border_style` → `eff_border`
- Line 448-452: `active_border_style` → `eff_active`, `border_style` → `eff_border`
- Line 470-474: `active_border_style` → `eff_active`, `border_style` → `eff_border`

Since when `!window_focused` both `eff_border` and `eff_active` are the same value (`unfocused_border_style`), all borders collapse to the same muted style automatically.

- [ ] **Step 5: Verify it compiles**

Run: `cargo check 2>&1 | head -5`
Expected: clean

- [ ] **Step 6: Commit**

```bash
git add src/rendering.rs
git commit -m "feat: three-way border style selection based on window focus"
```

---

### Task 5: Dim status bar when unfocused

**Files:**
- Modify: `src/app.rs:685-687` (status bar Paragraph creation)

- [ ] **Step 1: Apply DIM modifier to status bar when unfocused**

In `src/app.rs`, find the status bar rendering (line 685):

```rust
            let status_bar = Paragraph::new(Line::from(combined)).style(base_status_style);
```

Replace with:

```rust
            let final_status_style = if app.window_focused {
                base_status_style
            } else {
                base_status_style.add_modifier(Modifier::DIM)
            };
            let status_bar = Paragraph::new(Line::from(combined)).style(final_status_style);
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -5`
Expected: clean

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat: dim status bar when window is unfocused"
```

---

### Task 6: Tests

**Files:**
- Create: `tests/focus_border_tests.rs`

- [ ] **Step 1: Write unit tests for three-way style resolution**

Create `tests/focus_border_tests.rs`:

```rust
//! Tests for three-state pane focus border rendering.

use ratatui::style::{Color, Modifier, Style};

/// Simulates the three-way effective style resolution from rendering.rs.
/// This mirrors the logic: unfocused overrides everything.
fn effective_styles(
    border: Style,
    active: Style,
    unfocused: Style,
    window_focused: bool,
) -> (Style, Style) {
    let eff_border = if window_focused { border } else { unfocused };
    let eff_active = if window_focused { active } else { unfocused };
    (eff_border, eff_active)
}

#[test]
fn focused_active_pane_gets_active_style() {
    let border = Style::default();
    let active = Style::default().fg(Color::Green);
    let unfocused = Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM);

    let (eff_border, eff_active) = effective_styles(border, active, unfocused, true);

    assert_eq!(eff_active, active, "active pane should use active_border_style when focused");
    assert_eq!(eff_border, border, "inactive pane should use border_style when focused");
    assert_ne!(eff_active, eff_border, "active and inactive should differ when focused");
}

#[test]
fn unfocused_all_borders_same() {
    let border = Style::default();
    let active = Style::default().fg(Color::Green);
    let unfocused = Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM);

    let (eff_border, eff_active) = effective_styles(border, active, unfocused, false);

    assert_eq!(eff_border, unfocused, "inactive border should use unfocused style");
    assert_eq!(eff_active, unfocused, "active border should also use unfocused style");
    assert_eq!(eff_border, eff_active, "all borders should be identical when unfocused");
}

#[test]
fn unfocused_style_has_dim_modifier() {
    let unfocused = Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM);

    let (eff_border, _) = effective_styles(
        Style::default(),
        Style::default().fg(Color::Green),
        unfocused,
        false,
    );

    assert!(
        eff_border.add_modifier == Modifier::DIM || eff_border.to_string().contains("DIM")
            || eff_border == unfocused,
        "unfocused borders should carry DIM modifier"
    );
}

#[test]
fn custom_unfocused_style_respected() {
    let border = Style::default();
    let active = Style::default().fg(Color::Green);
    let custom_unfocused = Style::default().fg(Color::Red).bg(Color::Black);

    let (eff_border, eff_active) = effective_styles(border, active, custom_unfocused, false);

    assert_eq!(eff_border, custom_unfocused);
    assert_eq!(eff_active, custom_unfocused);
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --test focus_border_tests -- --nocapture`
Expected: all 4 tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/focus_border_tests.rs
git commit -m "test: three-state focus border style resolution"
```

---

### Task 7: Full build + lint check

- [ ] **Step 1: Run full CI check**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: all pass

- [ ] **Step 2: Fix any clippy warnings or test failures**

Address any issues found in step 1.

- [ ] **Step 3: Final commit if fixes needed**

```bash
git add -A
git commit -m "fix: clippy and test fixes for focus border feature"
```
