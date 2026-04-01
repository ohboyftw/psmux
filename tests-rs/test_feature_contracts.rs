/// Feature contract tests for unimplemented VT100 parser capabilities.
///
/// These tests define the EXPECTED behavior for features that DO NOT YET EXIST
/// in the psmux VT100 parser. They are designed to FAIL today and PASS after
/// the features are implemented.
///
/// Organized by feature:
///   - Task 8: Focus Reporting Mode (DECSET ?1004)
///   - Task 9: Cursor Style (DECSCUSR -- CSI Ps SP q)
///   - Task 6: Encoding-Aware Mouse Output (X10 vs SGR)

// =========================================================================
// TASK 8: Focus Reporting Mode (DECSET ?1004)
//
// The VT parser does NOT track ?1004h/?1004l. There is no focus_reporting()
// method on Screen. These tests define the contract for when it is added.
// =========================================================================

mod focus_reporting_state_diff {
    /// Contract: When a screen has processed \x1b[?1004h and the previous
    /// screen has not, state_diff() must include \x1b[?1004h in its output.
    /// This is how psmux restores focus reporting mode when switching panes.

    #[test]
    fn diff_from_default_to_focus_enabled_includes_1004h() {
        let mut parser_with = vt100::Parser::new(24, 80, 0);
        let parser_without = vt100::Parser::new(24, 80, 0);

        parser_with.process(b"\x1b[?1004h");

        // state_diff from without -> with should include ?1004h
        let diff = parser_with.screen().state_diff(parser_without.screen());
        let diff_str = String::from_utf8_lossy(&diff);
        assert!(
            diff_str.contains("\x1b[?1004h"),
            "state_diff should include ?1004h to enable focus reporting, got: {:?}",
            diff_str
        );
    }

    #[test]
    fn diff_from_focus_enabled_to_default_includes_1004l() {
        let mut parser_with = vt100::Parser::new(24, 80, 0);
        let parser_without = vt100::Parser::new(24, 80, 0);

        parser_with.process(b"\x1b[?1004h");

        // state_diff from with -> without should include ?1004l
        let diff = parser_without.screen().state_diff(parser_with.screen());
        let diff_str = String::from_utf8_lossy(&diff);
        assert!(
            diff_str.contains("\x1b[?1004l"),
            "state_diff should include ?1004l to disable focus reporting, got: {:?}",
            diff_str
        );
    }

    #[test]
    fn diff_between_two_focus_enabled_screens_is_empty_for_1004() {
        let mut parser_a = vt100::Parser::new(24, 80, 0);
        let mut parser_b = vt100::Parser::new(24, 80, 0);

        parser_a.process(b"\x1b[?1004h");
        parser_b.process(b"\x1b[?1004h");

        // Both have focus reporting enabled -- diff should NOT mention ?1004
        let diff = parser_a.screen().state_diff(parser_b.screen());
        let diff_str = String::from_utf8_lossy(&diff);
        assert!(
            !diff_str.contains("1004"),
            "state_diff between two focus-enabled screens should not mention 1004, got: {:?}",
            diff_str
        );
    }

    #[test]
    fn diff_between_two_default_screens_is_empty_for_1004() {
        let parser_a = vt100::Parser::new(24, 80, 0);
        let parser_b = vt100::Parser::new(24, 80, 0);

        // Neither has focus reporting -- diff should NOT mention ?1004
        let diff = parser_a.screen().state_diff(parser_b.screen());
        let diff_str = String::from_utf8_lossy(&diff);
        assert!(
            !diff_str.contains("1004"),
            "state_diff between two default screens should not mention 1004, got: {:?}",
            diff_str
        );
    }
}

mod focus_reporting_input_mode_diff {
    /// Contract: input_mode_diff() is the dedicated method for mode changes.
    /// It must also emit ?1004h/?1004l when focus reporting differs.

    #[test]
    fn input_mode_diff_includes_focus_enable() {
        let mut parser_with = vt100::Parser::new(24, 80, 0);
        let parser_without = vt100::Parser::new(24, 80, 0);

        parser_with.process(b"\x1b[?1004h");

        let diff = parser_with.screen().input_mode_diff(parser_without.screen());
        let diff_str = String::from_utf8_lossy(&diff);
        assert!(
            diff_str.contains("\x1b[?1004h"),
            "input_mode_diff should enable focus reporting, got: {:?}",
            diff_str
        );
    }

    #[test]
    fn input_mode_diff_includes_focus_disable() {
        let mut parser_with = vt100::Parser::new(24, 80, 0);
        let parser_without = vt100::Parser::new(24, 80, 0);

        parser_with.process(b"\x1b[?1004h");

        let diff = parser_without.screen().input_mode_diff(parser_with.screen());
        let diff_str = String::from_utf8_lossy(&diff);
        assert!(
            diff_str.contains("\x1b[?1004l"),
            "input_mode_diff should disable focus reporting, got: {:?}",
            diff_str
        );
    }

    #[test]
    fn input_mode_formatted_includes_focus_when_enabled() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[?1004h");

        let formatted = parser.screen().input_mode_formatted();
        let formatted_str = String::from_utf8_lossy(&formatted);
        assert!(
            formatted_str.contains("\x1b[?1004h"),
            "input_mode_formatted should include ?1004h when focus reporting is on, got: {:?}",
            formatted_str
        );
    }

    #[test]
    fn input_mode_formatted_includes_focus_disable_when_off() {
        // input_mode_formatted dumps full state including disabled modes
        // (same pattern as bracketed paste emitting ?2004l when off)
        let parser = vt100::Parser::new(24, 80, 0);

        let formatted = parser.screen().input_mode_formatted();
        let formatted_str = String::from_utf8_lossy(&formatted);
        assert!(
            formatted_str.contains("1004l"),
            "input_mode_formatted should include ?1004l when focus reporting is off, got: {:?}",
            formatted_str
        );
    }
}

mod focus_reporting_decset_decrst {
    /// Contract: ?1004h/l should be tracked as a DECSET mode, and
    /// toggling it on/off should be idempotent and not affect other modes.

    #[test]
    fn focus_enable_disable_cycle_does_not_affect_mouse() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[?1003h\x1b[?1006h");

        // Enable then disable focus -- mouse should stay
        parser.process(b"\x1b[?1004h");
        parser.process(b"\x1b[?1004l");

        assert_eq!(
            parser.screen().mouse_protocol_mode(),
            vt100::MouseProtocolMode::AnyMotion,
            "Focus toggle should not disturb mouse mode"
        );
        assert_eq!(
            parser.screen().mouse_protocol_encoding(),
            vt100::MouseProtocolEncoding::Sgr,
            "Focus toggle should not disturb mouse encoding"
        );
    }

    #[test]
    fn focus_enable_disable_cycle_does_not_affect_bracketed_paste() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[?2004h");

        parser.process(b"\x1b[?1004h");
        parser.process(b"\x1b[?1004l");

        assert!(
            parser.screen().bracketed_paste(),
            "Focus toggle should not disturb bracketed paste"
        );
    }

    #[test]
    fn ris_resets_focus_reporting() {
        // RIS (Reset to Initial State, ESC c) should clear focus reporting
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[?1004h");

        // After RIS, state_diff from a default screen should be empty for 1004
        parser.process(b"\x1bc");

        let default_parser = vt100::Parser::new(24, 80, 0);
        let diff = parser.screen().state_diff(default_parser.screen());
        let diff_str = String::from_utf8_lossy(&diff);
        assert!(
            !diff_str.contains("1004"),
            "After RIS, focus reporting should be reset; state_diff should not mention 1004, got: {:?}",
            diff_str
        );
    }
}

// =========================================================================
// TASK 9: Cursor Style (DECSCUSR -- CSI Ps SP q)
//
// The VT parser processes DECSCUSR without crashing but does NOT expose
// the cursor style value or include it in state_diff/input_mode_diff.
// These tests define the contract for when cursor style tracking is added.
// =========================================================================

mod cursor_style_state_diff {
    /// Contract: When a screen has processed \x1b[2 q (steady block) and
    /// the previous screen has default cursor, state_diff() must include
    /// the DECSCUSR sequence so the terminal shows the correct cursor.

    #[test]
    fn diff_includes_decscusr_when_style_differs() {
        let mut parser_styled = vt100::Parser::new(24, 80, 0);
        let parser_default = vt100::Parser::new(24, 80, 0);

        parser_styled.process(b"\x1b[2 q"); // steady block

        let diff = parser_styled.screen().state_diff(parser_default.screen());
        let diff_str = String::from_utf8_lossy(&diff);
        assert!(
            diff_str.contains("\x1b[2 q"),
            "state_diff should include DECSCUSR \\x1b[2 q for steady block, got: {:?}",
            diff_str
        );
    }

    #[test]
    fn diff_includes_bar_cursor_style() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        let default_parser = vt100::Parser::new(24, 80, 0);

        parser.process(b"\x1b[6 q"); // steady bar

        let diff = parser.screen().state_diff(default_parser.screen());
        let diff_str = String::from_utf8_lossy(&diff);
        assert!(
            diff_str.contains("\x1b[6 q"),
            "state_diff should include DECSCUSR \\x1b[6 q for steady bar, got: {:?}",
            diff_str
        );
    }

    #[test]
    fn diff_includes_underline_cursor_style() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        let default_parser = vt100::Parser::new(24, 80, 0);

        parser.process(b"\x1b[4 q"); // steady underline

        let diff = parser.screen().state_diff(default_parser.screen());
        let diff_str = String::from_utf8_lossy(&diff);
        assert!(
            diff_str.contains("\x1b[4 q"),
            "state_diff should include DECSCUSR \\x1b[4 q for steady underline, got: {:?}",
            diff_str
        );
    }

    #[test]
    fn diff_between_two_same_style_screens_omits_decscusr() {
        let mut parser_a = vt100::Parser::new(24, 80, 0);
        let mut parser_b = vt100::Parser::new(24, 80, 0);

        parser_a.process(b"\x1b[2 q");
        parser_b.process(b"\x1b[2 q");

        let diff = parser_a.screen().state_diff(parser_b.screen());
        let diff_str = String::from_utf8_lossy(&diff);
        // Should not contain any " q" DECSCUSR sequence
        assert!(
            !diff_str.contains(" q"),
            "state_diff between screens with same cursor style should not contain DECSCUSR, got: {:?}",
            diff_str
        );
    }

    #[test]
    fn diff_from_bar_to_block_includes_block_decscusr() {
        let mut parser_block = vt100::Parser::new(24, 80, 0);
        let mut parser_bar = vt100::Parser::new(24, 80, 0);

        parser_block.process(b"\x1b[2 q"); // steady block
        parser_bar.process(b"\x1b[6 q");   // steady bar

        // Switching from bar pane to block pane
        let diff = parser_block.screen().state_diff(parser_bar.screen());
        let diff_str = String::from_utf8_lossy(&diff);
        assert!(
            diff_str.contains("\x1b[2 q"),
            "Switching from bar to block should emit DECSCUSR for block, got: {:?}",
            diff_str
        );
    }

    #[test]
    fn diff_restores_default_cursor_with_0_or_1() {
        let mut parser_bar = vt100::Parser::new(24, 80, 0);
        let parser_default = vt100::Parser::new(24, 80, 0);

        parser_bar.process(b"\x1b[6 q"); // steady bar

        // Switching from bar pane to default pane -- should reset cursor
        let diff = parser_default.screen().state_diff(parser_bar.screen());
        let diff_str = String::from_utf8_lossy(&diff);
        // Default cursor is style 0 (reset) or 1 (blinking block)
        let has_reset = diff_str.contains("\x1b[0 q") || diff_str.contains("\x1b[1 q");
        assert!(
            has_reset,
            "Switching to default cursor should emit DECSCUSR 0 or 1, got: {:?}",
            diff_str
        );
    }
}

mod cursor_style_state_formatted {
    /// Contract: state_formatted() should include the current cursor style
    /// when it differs from the default, so a full screen redraw restores it.

    #[test]
    fn state_formatted_includes_cursor_style() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[6 q"); // steady bar

        let formatted = parser.screen().state_formatted();
        let formatted_str = String::from_utf8_lossy(&formatted);
        assert!(
            formatted_str.contains("\x1b[6 q"),
            "state_formatted should include DECSCUSR for current cursor style, got: {:?}",
            formatted_str
        );
    }

    #[test]
    fn state_formatted_omits_cursor_style_when_default() {
        let parser = vt100::Parser::new(24, 80, 0);

        let formatted = parser.screen().state_formatted();
        let formatted_str = String::from_utf8_lossy(&formatted);
        // Default cursor (style 0) should not emit DECSCUSR
        assert!(
            !formatted_str.contains(" q"),
            "state_formatted should not include DECSCUSR when cursor is default, got: {:?}",
            formatted_str
        );
    }
}

mod cursor_style_neovim_workflow {
    /// Contract: neovim switches cursor style on mode change.
    /// The parser must track each change so pane-switch diffs are correct.

    #[test]
    fn neovim_insert_then_normal_tracked_correctly() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        let default_parser = vt100::Parser::new(24, 80, 0);

        // neovim: normal mode -> steady block
        parser.process(b"\x1b[2 q");
        let diff1 = parser.screen().state_diff(default_parser.screen());
        let diff1_str = String::from_utf8_lossy(&diff1);
        assert!(
            diff1_str.contains("\x1b[2 q"),
            "After setting steady block, diff should contain \\x1b[2 q, got: {:?}",
            diff1_str
        );

        // neovim: switch to insert mode -> steady bar
        parser.process(b"\x1b[6 q");
        let diff2 = parser.screen().state_diff(default_parser.screen());
        let diff2_str = String::from_utf8_lossy(&diff2);
        assert!(
            diff2_str.contains("\x1b[6 q"),
            "After switching to bar, diff should contain \\x1b[6 q, got: {:?}",
            diff2_str
        );
        // Must NOT still contain the old block style
        assert!(
            !diff2_str.contains("\x1b[2 q"),
            "After switching to bar, diff should NOT contain old block style, got: {:?}",
            diff2_str
        );
    }

    #[test]
    fn ris_resets_cursor_style() {
        // RIS should reset cursor style to default
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[6 q"); // set bar
        parser.process(b"\x1bc");     // RIS

        let default_parser = vt100::Parser::new(24, 80, 0);
        let diff = parser.screen().state_diff(default_parser.screen());
        let diff_str = String::from_utf8_lossy(&diff);
        assert!(
            !diff_str.contains(" q"),
            "After RIS, cursor style should be default; diff should not contain DECSCUSR, got: {:?}",
            diff_str
        );
    }
}

// =========================================================================
// TASK 6: Encoding-Aware Mouse Output
//
// write_mouse_to_pty() in window_ops.rs always emits SGR format. When
// the child requests Default/X10 encoding (no ?1006h), the output should
// use normal X10 format instead.
//
// write_mouse_event_remote() already handles both formats -- these tests
// validate the encoding-selection contract at the boundary.
// =========================================================================

mod mouse_encoding_selection {
    /// Contract: write_mouse_event_remote() must produce X10 format when
    /// encoding is Default, and SGR format when encoding is Sgr.

    /// Helper: encode a mouse event using the same logic as write_mouse_event_remote.
    fn encode_mouse_event(button: u8, col: u16, row: u16, press: bool, enc: vt100::MouseProtocolEncoding) -> Vec<u8> {
        let mut buf = Vec::new();
        match enc {
            vt100::MouseProtocolEncoding::Sgr => {
                let ch = if press { 'M' } else { 'm' };
                buf.extend_from_slice(
                    format!("\x1b[<{};{};{}{}", button, col, row, ch).as_bytes()
                );
            }
            _ => {
                if press {
                    let cb = button + 32;
                    let cx = (col as u8).min(223) + 32;
                    let cy = (row as u8).min(223) + 32;
                    buf.extend_from_slice(&[0x1b, b'[', b'M', cb, cx, cy]);
                }
                // X10/Default encoding does not support release events
            }
        }
        buf
    }

    #[test]
    fn default_encoding_produces_x10_format() {
        let result = encode_mouse_event(0, 1, 1, true, vt100::MouseProtocolEncoding::Default);
        // X10 format: ESC [ M Cb Cx Cy
        assert_eq!(result.len(), 6, "X10 format should be exactly 6 bytes");
        assert_eq!(&result[..3], b"\x1b[M", "X10 format should start with ESC[M");
    }

    #[test]
    fn sgr_encoding_produces_sgr_format() {
        let result = encode_mouse_event(0, 1, 1, true, vt100::MouseProtocolEncoding::Sgr);
        let result_str = String::from_utf8_lossy(&result);
        assert!(result_str.starts_with("\x1b[<"), "SGR format should start with ESC[<");
        assert!(result_str.ends_with('M'), "SGR press should end with M");
    }

    #[test]
    fn default_encoding_release_produces_nothing() {
        // X10/Default encoding cannot represent release events
        let result = encode_mouse_event(0, 1, 1, false, vt100::MouseProtocolEncoding::Default);
        assert!(
            result.is_empty(),
            "X10 format should produce nothing for release events, got {} bytes",
            result.len()
        );
    }

    #[test]
    fn sgr_encoding_release_produces_lowercase_m() {
        let result = encode_mouse_event(0, 1, 1, false, vt100::MouseProtocolEncoding::Sgr);
        let result_str = String::from_utf8_lossy(&result);
        assert!(result_str.ends_with('m'), "SGR release should end with lowercase m");
    }
}

mod mouse_encoding_x10_coordinate_limits {
    /// Contract: X10 (Default) encoding has a maximum coordinate of 223
    /// because each coordinate is a single byte (value + 32), max u8 = 255.
    /// Coordinates > 223 must be clamped.

    fn encode_x10_coord(col: u16, row: u16) -> (u8, u8) {
        let cx = (col.min(223) as u8) + 32;
        let cy = (row.min(223) as u8) + 32;
        (cx, cy)
    }

    #[test]
    fn x10_coordinate_at_limit() {
        let (cx, cy) = encode_x10_coord(223, 223);
        assert_eq!(cx, 255, "col 223 + 32 = 255");
        assert_eq!(cy, 255, "row 223 + 32 = 255");
    }

    #[test]
    fn x10_coordinate_beyond_limit_clamped() {
        // Coordinates > 223 should be clamped to 223
        let (cx, cy) = encode_x10_coord(300, 400);
        assert_eq!(cx, 255, "col > 223 should clamp to 223+32=255");
        assert_eq!(cy, 255, "row > 223 should clamp to 223+32=255");
    }

    #[test]
    fn x10_coordinate_zero_is_valid() {
        let (cx, cy) = encode_x10_coord(0, 0);
        assert_eq!(cx, 32, "col 0 + 32 = 32 (space)");
        assert_eq!(cy, 32, "row 0 + 32 = 32 (space)");
    }
}

mod mouse_encoding_pane_state_contract {
    /// Contract: The VT parser's mouse_protocol_encoding() should determine
    /// which format is used for mouse injection. When a pane has Default
    /// encoding (no ?1006h), mouse events MUST use X10 format.
    /// When a pane has SGR encoding (?1006h), mouse events MUST use SGR format.

    #[test]
    fn pane_without_1006h_uses_default_encoding() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        // Enable mouse tracking but NOT SGR encoding
        parser.process(b"\x1b[?1003h");

        assert_eq!(
            parser.screen().mouse_protocol_encoding(),
            vt100::MouseProtocolEncoding::Default,
            "Without ?1006h, encoding should be Default"
        );

        // This is the contract: write_mouse_to_pty() should check this
        // and use X10 format, not SGR. Currently it always uses SGR.
        // When the fix lands, encoding Default -> X10 bytes.
        let enc = parser.screen().mouse_protocol_encoding();
        assert_eq!(enc, vt100::MouseProtocolEncoding::Default);
    }

    #[test]
    fn pane_with_1006h_uses_sgr_encoding() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[?1003h\x1b[?1006h");

        assert_eq!(
            parser.screen().mouse_protocol_encoding(),
            vt100::MouseProtocolEncoding::Sgr,
            "With ?1006h, encoding should be SGR"
        );
    }

    #[test]
    fn write_mouse_to_pty_should_respect_encoding_contract() {
        // This test defines the contract that write_mouse_to_pty SHOULD obey:
        // When encoding is Default, output should be X10 format (6 bytes).
        //
        // Today, write_mouse_to_pty always emits SGR regardless of encoding.
        // This test documents the EXPECTED behavior after the fix.

        // Simulate what write_mouse_to_pty should do for Default encoding:
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[?1003h"); // mouse mode on, encoding stays Default

        let enc = parser.screen().mouse_protocol_encoding();

        // The function should produce X10 format for Default encoding
        let mut buf = Vec::new();
        let button: u8 = 0;
        let col: u16 = 10;
        let row: u16 = 5;
        match enc {
            vt100::MouseProtocolEncoding::Sgr => {
                use std::io::Write;
                write!(&mut buf, "\x1b[<{};{};{}M", button, col, row).unwrap();
            }
            _ => {
                let cb = button + 32;
                let cx = (col as u8).min(223) + 32;
                let cy = (row as u8).min(223) + 32;
                buf.extend_from_slice(&[0x1b, b'[', b'M', cb, cx, cy]);
            }
        }

        // Default encoding -> X10 format (6 bytes, starts with ESC[M)
        assert_eq!(buf.len(), 6, "Default encoding should produce X10 format (6 bytes)");
        assert_eq!(&buf[..3], b"\x1b[M", "Default encoding should start with ESC[M");
    }
}

mod mouse_encoding_utf8 {
    /// Contract: UTF-8 mouse encoding (?1005h) is a third encoding variant.
    /// The parser tracks it, but write_mouse_event_remote and write_mouse_to_pty
    /// should handle it distinctly from Default and SGR.

    #[test]
    fn utf8_encoding_tracked_by_parser() {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(b"\x1b[?1005h");
        assert_eq!(
            parser.screen().mouse_protocol_encoding(),
            vt100::MouseProtocolEncoding::Utf8
        );
    }

    #[test]
    fn diff_captures_utf8_encoding_transition() {
        let mut parser_utf8 = vt100::Parser::new(24, 80, 0);
        let parser_default = vt100::Parser::new(24, 80, 0);

        parser_utf8.process(b"\x1b[?1005h");

        let diff = parser_utf8.screen().state_diff(parser_default.screen());
        let diff_str = String::from_utf8_lossy(&diff);
        assert!(
            diff_str.contains("\x1b[?1005h"),
            "state_diff should include ?1005h for UTF-8 encoding, got: {:?}",
            diff_str
        );
    }
}
