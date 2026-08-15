use super::*;

// ── parse_osc7_uri tests ──────────────────────────────────

#[test]
fn osc7_full_uri_with_hostname() {
    assert_eq!(
        parse_osc7_uri("file://myhost/home/user/project"),
        "/home/user/project"
    );
}

#[test]
fn osc7_localhost() {
    assert_eq!(parse_osc7_uri("file://localhost/home/user"), "/home/user");
}

#[test]
fn osc7_empty_hostname() {
    assert_eq!(parse_osc7_uri("file:///home/user"), "/home/user");
}

#[test]
fn osc7_bare_path_no_scheme() {
    assert_eq!(parse_osc7_uri("/home/user/code"), "/home/user/code");
}

#[test]
fn osc7_percent_encoded_spaces() {
    assert_eq!(
        parse_osc7_uri("file:///home/user/my%20project"),
        "/home/user/my project"
    );
}

#[test]
fn osc7_percent_encoded_special_chars() {
    assert_eq!(
        parse_osc7_uri("file:///path/%23hash%25pct"),
        "/path/#hash%pct"
    );
}

#[test]
fn osc7_windows_path_via_uri() {
    // WezTerm-style: file://hostname/C:/Users/foo
    assert_eq!(
        parse_osc7_uri("file://DESKTOP-ABC/C:/Users/foo"),
        "/C:/Users/foo"
    );
}

#[test]
fn osc7_empty_string() {
    assert_eq!(parse_osc7_uri(""), "");
}

#[test]
fn osc7_file_no_slash_after_host() {
    // Malformed: file://hostname-only (no path)
    assert_eq!(parse_osc7_uri("file://hostname-only"), "hostname-only");
}

// ── OSC title tests ──────────────────────────────────────────

#[test]
fn screen_title_initially_empty() {
    let screen = Screen::new(crate::grid::Size { rows: 24, cols: 80 }, 0);
    assert_eq!(screen.title(), "");
}

#[test]
fn screen_set_title_from_utf8() {
    let mut screen = Screen::new(crate::grid::Size { rows: 24, cols: 80 }, 0);
    screen.set_title(b"my terminal");
    assert_eq!(screen.title(), "my terminal");
}

#[test]
fn screen_set_title_overwrites() {
    let mut screen = Screen::new(crate::grid::Size { rows: 24, cols: 80 }, 0);
    screen.set_title(b"first");
    screen.set_title(b"second");
    assert_eq!(screen.title(), "second");
}

#[test]
fn screen_set_title_empty_string() {
    let mut screen = Screen::new(crate::grid::Size { rows: 24, cols: 80 }, 0);
    screen.set_title(b"something");
    screen.set_title(b"");
    assert_eq!(screen.title(), "");
}

#[test]
fn screen_set_title_invalid_utf8_ignored() {
    let mut screen = Screen::new(crate::grid::Size { rows: 24, cols: 80 }, 0);
    screen.set_title(b"good");
    screen.set_title(&[0xff, 0xfe, 0xfd]);
    // Invalid UTF-8 should be ignored, old title preserved
    assert_eq!(screen.title(), "good");
}

// ── percent_decode tests ──────────────────────────────────

#[test]
fn decode_no_encoding() {
    assert_eq!(percent_decode("/simple/path"), "/simple/path");
}

#[test]
fn decode_space() {
    assert_eq!(percent_decode("/my%20path"), "/my path");
}

#[test]
fn decode_mixed_case_hex() {
    assert_eq!(percent_decode("%2f%2F"), "//");
}

#[test]
fn decode_invalid_hex_passthrough() {
    assert_eq!(percent_decode("%ZZ"), "%ZZ");
}

#[test]
fn decode_truncated_percent() {
    assert_eq!(percent_decode("trail%2"), "trail%2");
}

// ── Screen::set_path / path() integration ─────────────────

#[test]
fn screen_path_initially_none() {
    let s = Screen::new(crate::grid::Size { rows: 24, cols: 80 }, 0);
    assert!(s.path().is_none());
}

#[test]
fn screen_set_path_from_osc7() {
    let mut s = Screen::new(crate::grid::Size { rows: 24, cols: 80 }, 0);
    s.set_path(b"file:///home/user/code");
    assert_eq!(s.path(), Some("/home/user/code"));
}

#[test]
fn screen_set_path_overwrites() {
    let mut s = Screen::new(crate::grid::Size { rows: 24, cols: 80 }, 0);
    s.set_path(b"file:///first");
    s.set_path(b"file:///second");
    assert_eq!(s.path(), Some("/second"));
}

#[test]
fn screen_set_path_ignores_invalid_utf8() {
    let mut s = Screen::new(crate::grid::Size { rows: 24, cols: 80 }, 0);
    s.set_path(&[0xff, 0xfe, 0xfd]);
    assert!(s.path().is_none());
}

// ── Full parser round-trip via VTE ─────────────────────────

#[test]
fn parser_osc7_roundtrip() {
    let mut parser = crate::Parser::new(24, 80, 0);
    // OSC 7 ; file:///tmp/test ST
    parser.process(b"\x1b]7;file:///tmp/test\x1b\\");
    assert_eq!(parser.screen().path(), Some("/tmp/test"));
}

#[test]
fn parser_osc7_bel_terminated() {
    let mut parser = crate::Parser::new(24, 80, 0);
    // OSC 7 ; file://host/path BEL
    parser.process(b"\x1b]7;file://host/home/user\x07");
    assert_eq!(parser.screen().path(), Some("/home/user"));
}

#[test]
fn parser_osc7_with_percent_encoding() {
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.process(b"\x1b]7;file:///home/user/my%20project\x07");
    assert_eq!(parser.screen().path(), Some("/home/user/my project"));
}

#[test]
fn parser_osc7_updates_on_cd() {
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.process(b"\x1b]7;file:///first/dir\x07");
    assert_eq!(parser.screen().path(), Some("/first/dir"));
    parser.process(b"\x1b]7;file:///second/dir\x07");
    assert_eq!(parser.screen().path(), Some("/second/dir"));
}

#[test]
fn parser_other_osc_does_not_affect_path() {
    let mut parser = crate::Parser::new(24, 80, 0);
    // OSC 0 (set title) should not touch path
    parser.process(b"\x1b]0;my-title\x07");
    assert!(parser.screen().path().is_none());
}

// ── Squelch signal tests ──────────────────────────────────

#[test]
fn squelch_initially_not_set() {
    let s = Screen::new(crate::grid::Size { rows: 24, cols: 80 }, 0);
    assert!(!s.squelch_cleared());
}

#[test]
fn squelch_pending_initially_false() {
    let mut s = Screen::new(crate::grid::Size { rows: 24, cols: 80 }, 0);
    // take should return false when nothing was set
    assert!(!s.take_squelch_cleared());
}

#[test]
fn squelch_armed_then_csi_2j_fires_signal() {
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.screen_mut().set_squelch_clear_pending(true);
    // CSI 2J = erase display (mode 2)
    parser.process(b"\x1b[2J");
    assert!(parser.screen().squelch_cleared());
}

#[test]
fn squelch_armed_then_csi_3j_fires_signal() {
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.screen_mut().set_squelch_clear_pending(true);
    // CSI 3J = clear scrollback (mode 3)
    parser.process(b"\x1b[3J");
    assert!(parser.screen().squelch_cleared());
}

#[test]
fn squelch_not_armed_csi_2j_does_not_fire() {
    let mut parser = crate::Parser::new(24, 80, 0);
    // Do NOT arm squelch, just send CSI 2J
    parser.process(b"\x1b[2J");
    assert!(!parser.screen().squelch_cleared());
}

#[test]
fn squelch_not_armed_csi_3j_does_not_fire() {
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.process(b"\x1b[3J");
    assert!(!parser.screen().squelch_cleared());
}

#[test]
fn squelch_take_clears_flag() {
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.screen_mut().set_squelch_clear_pending(true);
    parser.process(b"\x1b[2J");
    assert!(parser.screen().squelch_cleared());
    // take should return true and clear
    assert!(parser.screen_mut().take_squelch_cleared());
    // second take should return false
    assert!(!parser.screen_mut().take_squelch_cleared());
}

#[test]
fn squelch_fires_only_once_per_arm() {
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.screen_mut().set_squelch_clear_pending(true);
    parser.process(b"\x1b[2J");
    assert!(parser.screen_mut().take_squelch_cleared());

    // Second CSI 2J without re-arming should not fire
    parser.process(b"\x1b[2J");
    assert!(!parser.screen().squelch_cleared());
}

#[test]
fn squelch_rearm_fires_again() {
    let mut parser = crate::Parser::new(24, 80, 0);
    // First arm + fire
    parser.screen_mut().set_squelch_clear_pending(true);
    parser.process(b"\x1b[2J");
    assert!(parser.screen_mut().take_squelch_cleared());

    // Re-arm + fire
    parser.screen_mut().set_squelch_clear_pending(true);
    parser.process(b"\x1b[3J");
    assert!(parser.screen_mut().take_squelch_cleared());
}

#[test]
fn squelch_csi_0j_does_not_fire() {
    // CSI 0J = erase from cursor to end; should NOT trigger squelch
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.screen_mut().set_squelch_clear_pending(true);
    parser.process(b"\x1b[0J");
    assert!(!parser.screen().squelch_cleared());
}

#[test]
fn squelch_csi_1j_does_not_fire() {
    // CSI 1J = erase from start to cursor; should NOT trigger squelch
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.screen_mut().set_squelch_clear_pending(true);
    parser.process(b"\x1b[1J");
    assert!(!parser.screen().squelch_cleared());
}

#[test]
fn squelch_regular_text_does_not_fire() {
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.screen_mut().set_squelch_clear_pending(true);
    parser.process(b"Hello world\r\n");
    assert!(!parser.screen().squelch_cleared());
    // Pending should still be armed
    parser.process(b"\x1b[2J");
    assert!(parser.screen().squelch_cleared());
}

#[test]
fn squelch_mixed_escape_sequences_before_clear() {
    // Simulate ConPTY output: cursor moves, text, then cls output (CSI 3J)
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.screen_mut().set_squelch_clear_pending(true);
    // Cursor move, some text, color, then the clear
    parser.process(b"\x1b[1;1H"); // cursor home
    parser.process(b"PS C:\\> cd 'C:\\temp'; cls\r\n"); // injected command echo
    parser.process(b"\x1b[0m"); // reset attributes
                                // Signal should NOT have fired yet (no CSI 2J/3J)
    assert!(!parser.screen().squelch_cleared());
    // Now the actual clear arrives
    parser.process(b"\x1b[3J");
    assert!(parser.screen().squelch_cleared());
}

#[test]
fn squelch_disarm_prevents_fire() {
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.screen_mut().set_squelch_clear_pending(true);
    // Disarm before CSI 2J arrives
    parser.screen_mut().set_squelch_clear_pending(false);
    parser.process(b"\x1b[2J");
    assert!(!parser.screen().squelch_cleared());
}

#[test]
fn squelch_csi_2j_then_3j_only_first_fires() {
    // If armed, first CSI 2J fires and disarms; the subsequent CSI 3J should not fire
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.screen_mut().set_squelch_clear_pending(true);
    parser.process(b"\x1b[2J");
    assert!(parser.screen_mut().take_squelch_cleared());
    // Now CSI 3J arrives but pending is already cleared
    parser.process(b"\x1b[3J");
    assert!(!parser.screen().squelch_cleared());
}

// ── issue #534: shrinking a row through a wide glyph ─────────

#[test]
fn issue534_shrink_wide_then_erase_in_line_does_not_panic() {
    // Exact repro from the report: 2x4 parser, write a wide CJK glyph, shrink
    // to one column (truncating away its continuation), then erase in line.
    // Before the fix this panicked with "attempt to subtract with overflow"
    // in Row::erase.
    let mut parser = crate::Parser::new(2, 4, 0);
    parser.process("\u{4E2D}".as_bytes());
    parser.screen_mut().set_size(2, 1);
    parser.process(b"\x1b[K");
    // The salvaged cell must not claim a width the row cannot hold.
    let cell = parser.screen().cell(0, 0).unwrap();
    assert!(!cell.is_wide(), "orphaned wide cell survived the shrink");
}

#[test]
fn issue534_shrink_wide_sweep_no_cell_claims_wider_than_row() {
    // Sweep every shrink target from 8 columns down to 1 after writing a wide
    // glyph: no cell may keep a wide flag its row cannot hold.
    for target in (1..=8u16).rev() {
        let mut parser = crate::Parser::new(2, 8, 0);
        parser.process("\u{4E2D}".as_bytes());
        parser.screen_mut().set_size(2, target);
        for col in 0..target {
            if let Some(cell) = parser.screen().cell(0, col) {
                assert!(
                    !cell.is_wide() || col + 1 < target,
                    "wide cell at col {col} with no continuation in a {target} column row"
                );
            }
        }
    }
}

#[test]
fn issue534_shrink_wide_then_erase_in_display_does_not_panic() {
    let mut parser = crate::Parser::new(2, 4, 0);
    parser.process("\u{4E2D}".as_bytes());
    parser.screen_mut().set_size(2, 1);
    // CSI 2J erases the whole display; must not underflow on the orphaned cell.
    parser.process(b"\x1b[2J");
    assert!(!parser.screen().cell(0, 0).unwrap().is_wide());
}

#[test]
fn issue534_shrink_wide_regrow_keeps_grid_coherent() {
    let mut parser = crate::Parser::new(2, 4, 0);
    parser.process("\u{4E2D}".as_bytes());
    parser.screen_mut().set_size(2, 1);
    parser.screen_mut().set_size(2, 4);
    // After regrowing, no cell should be flagged wide without a continuation.
    for col in 0..4 {
        if let Some(cell) = parser.screen().cell(0, col) {
            if cell.is_wide() {
                assert!(col + 1 < 4, "wide flag with no continuation after regrow");
            }
        }
    }
}

// ── issue #533: VS16 promotes a cell to wide ────────────────

#[test]
fn issue533_vs16_promotes_base_to_wide_and_advances_cursor() {
    // U+2733 is one column on its own, but U+2733 U+FE0F is two. Before the
    // fix the base settled at one column and the following columns drifted
    // left by one.
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.process("\u{2733}\u{FE0F}AB".as_bytes());
    let base = parser.screen().cell(0, 0).unwrap();
    assert!(base.is_wide(), "base cell not promoted to wide");
    assert!(parser.screen().cell(0, 1).unwrap().is_wide_continuation());
    assert_eq!(parser.screen().cell(0, 2).unwrap().contents(), "A");
    assert_eq!(parser.screen().cell(0, 3).unwrap().contents(), "B");
    let (_, col) = parser.screen().cursor_position();
    assert_eq!(col, 4, "cursor did not advance past the promoted glyph");
}

#[test]
fn issue533_vs16_split_across_process_calls() {
    // A PTY read splitting the base and the selector is routine; promotion
    // must still apply because it acts on the cell already in the grid.
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.process("\u{2733}".as_bytes());
    parser.process("\u{FE0F}".as_bytes());
    parser.process(b"A");
    assert!(parser.screen().cell(0, 0).unwrap().is_wide());
    assert!(parser.screen().cell(0, 1).unwrap().is_wide_continuation());
    assert_eq!(parser.screen().cell(0, 2).unwrap().contents(), "A");
}

#[test]
fn issue533_vs15_does_not_promote() {
    // VS15 (U+FE0E) requests text presentation: the base stays one column.
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.process("\u{2733}\u{FE0E}A".as_bytes());
    assert!(!parser.screen().cell(0, 0).unwrap().is_wide());
    assert_eq!(parser.screen().cell(0, 1).unwrap().contents(), "A");
}

#[test]
fn issue533_redundant_vs16_on_already_wide_emoji_stays_two() {
    // An already-wide emoji plus a redundant VS16 must not grow to four columns.
    let mut parser = crate::Parser::new(24, 80, 0);
    parser.process("\u{1F4DB}\u{FE0F}A".as_bytes());
    assert!(parser.screen().cell(0, 0).unwrap().is_wide());
    assert!(parser.screen().cell(0, 1).unwrap().is_wide_continuation());
    assert_eq!(parser.screen().cell(0, 2).unwrap().contents(), "A");
    let (_, col) = parser.screen().cursor_position();
    assert_eq!(col, 3);
}

#[test]
fn issue533_vs16_promoted_cell_survives_truncation() {
    // A #533-promoted wide cell must survive the same truncation path as a
    // CJK glyph (#534 interaction).
    let mut parser = crate::Parser::new(2, 4, 0);
    parser.process("\u{2733}\u{FE0F}".as_bytes());
    assert!(parser.screen().cell(0, 0).unwrap().is_wide());
    parser.screen_mut().set_size(2, 1);
    assert!(
        !parser.screen().cell(0, 0).unwrap().is_wide(),
        "promoted wide cell kept its flag after being shrunk to one column"
    );
}
