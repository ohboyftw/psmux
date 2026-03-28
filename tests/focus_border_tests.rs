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
    let unfocused = Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::DIM);

    let (eff_border, eff_active) = effective_styles(border, active, unfocused, true);

    assert_eq!(
        eff_active, active,
        "active pane should use active_border_style when focused"
    );
    assert_eq!(
        eff_border, border,
        "inactive pane should use border_style when focused"
    );
    assert_ne!(
        eff_active, eff_border,
        "active and inactive should differ when focused"
    );
}

#[test]
fn unfocused_all_borders_same() {
    let border = Style::default();
    let active = Style::default().fg(Color::Green);
    let unfocused = Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::DIM);

    let (eff_border, eff_active) = effective_styles(border, active, unfocused, false);

    assert_eq!(
        eff_border, unfocused,
        "inactive border should use unfocused style"
    );
    assert_eq!(
        eff_active, unfocused,
        "active border should also use unfocused style"
    );
    assert_eq!(
        eff_border, eff_active,
        "all borders should be identical when unfocused"
    );
}

#[test]
fn unfocused_style_carries_dim() {
    let unfocused = Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::DIM);

    let (eff_border, _) = effective_styles(
        Style::default(),
        Style::default().fg(Color::Green),
        unfocused,
        false,
    );

    assert_eq!(
        eff_border, unfocused,
        "unfocused borders should carry the full unfocused style"
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
