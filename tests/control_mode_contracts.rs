/// Golden transcript test for the control mode parser.
///
/// This test feeds a realistic tmux `-CC` transcript through the parser and
/// asserts that the expected message types are produced.
#[test]
fn test_golden_transcript() {
    let transcript = include_str!("fixtures/tmux_cc_session.txt");
    let mut parser = psmux::remote::parser::ControlModeParser::new();
    let messages: Vec<_> = transcript
        .lines()
        .filter_map(|line| parser.feed_line(line))
        .collect();

    assert!(
        !messages.is_empty(),
        "Golden transcript should produce messages"
    );

    // Should contain output notifications.
    assert!(messages.iter().any(
        |m| matches!(m, psmux::remote::protocol::ControlModeMessage::Output { .. })
    ));
    // Should contain window-add notifications.
    assert!(messages.iter().any(
        |m| matches!(m, psmux::remote::protocol::ControlModeMessage::WindowAdd { .. })
    ));
    // Should contain successful response blocks.
    assert!(messages.iter().any(|m| matches!(
        m,
        psmux::remote::protocol::ControlModeMessage::Response {
            success: true,
            ..
        }
    )));
    // Should contain layout-change notifications.
    assert!(messages.iter().any(
        |m| matches!(m, psmux::remote::protocol::ControlModeMessage::LayoutChange { .. })
    ));
    // Should contain session-changed notification.
    assert!(messages.iter().any(
        |m| matches!(m, psmux::remote::protocol::ControlModeMessage::SessionChanged { .. })
    ));
    // Should contain sessions-changed notification.
    assert!(messages
        .iter()
        .any(|m| matches!(m, psmux::remote::protocol::ControlModeMessage::SessionsChanged)));
    // Should contain window-pane-changed notification.
    assert!(messages.iter().any(
        |m| matches!(m, psmux::remote::protocol::ControlModeMessage::WindowPaneChanged { .. })
    ));
    // Should contain pane-mode-changed notification.
    assert!(messages.iter().any(
        |m| matches!(m, psmux::remote::protocol::ControlModeMessage::PaneModeChanged { .. })
    ));
}
