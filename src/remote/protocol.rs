//! tmux control mode protocol types and notification parser.
//!
//! When tmux is launched with `-CC`, it emits structured notifications
//! (lines beginning with `%`) that describe server-side events. This module
//! defines the [`ControlModeMessage`] enum covering every documented
//! notification type and provides [`parse_notification`] to convert a raw
//! line into a typed message.

use serde::{Deserialize, Serialize};

/// A parsed tmux control mode message.
///
/// Variants map 1-to-1 to the `%`-prefixed notifications described in
/// `tmux(1)` § CONTROL MODE.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ControlModeMessage {
    /// Response to a command sent by the client.
    Response {
        timestamp: u64,
        command_id: u64,
        flags: u32,
        body: Vec<String>,
        success: bool,
    },
    /// Pane produced output (octal-decoded).
    Output {
        pane_id: String,
        data: Vec<u8>,
    },
    /// Extended output with lag information.
    ExtendedOutput {
        pane_id: String,
        lag_ms: u64,
        data: Vec<u8>,
    },
    /// A window was added to the current session.
    WindowAdd {
        window_id: String,
    },
    /// A window was closed in the current session.
    WindowClose {
        window_id: String,
    },
    /// A window was renamed in the current session.
    WindowRenamed {
        window_id: String,
        new_name: String,
    },
    /// A window was added outside the current session.
    UnlinkedWindowAdd {
        window_id: String,
    },
    /// A window was closed outside the current session.
    UnlinkedWindowClose {
        window_id: String,
    },
    /// A window was renamed outside the current session.
    UnlinkedWindowRenamed {
        window_id: String,
        new_name: String,
    },
    /// The client switched to a different session.
    SessionChanged {
        session_id: String,
        session_name: String,
    },
    /// A session was renamed.
    SessionRenamed {
        session_id: String,
        new_name: String,
    },
    /// The list of sessions changed (created / destroyed).
    SessionsChanged,
    /// The active window of a session changed.
    SessionWindowChanged {
        session_id: String,
        window_id: String,
    },
    /// A client changed its attached session.
    ClientSessionChanged {
        client: String,
        session_id: String,
        session_name: String,
    },
    /// The active pane of a window changed.
    WindowPaneChanged {
        window_id: String,
        pane_id: String,
    },
    /// A pane's mode changed (e.g. copy mode entered/exited).
    PaneModeChanged {
        pane_id: String,
    },
    /// A window's layout changed.
    LayoutChange {
        window_id: String,
        layout_string: String,
    },
    /// A pane has been paused (output throttled).
    Pause {
        pane_id: String,
    },
    /// A paused pane has resumed.
    Continue {
        pane_id: String,
    },
    /// A user-defined subscription variable changed.
    SubscriptionChanged {
        name: String,
        value: String,
    },
    /// The control mode session is exiting.
    Exit {
        reason: Option<String>,
    },
}

/// Parse a single control mode notification line.
///
/// Returns `None` for empty lines, non-notification lines (those not starting
/// with `%`), and unrecognised notification types. This allows callers to
/// silently skip lines they don't understand, which is important for
/// forward-compatibility with newer tmux versions.
pub fn parse_notification(line: &str) -> Option<ControlModeMessage> {
    let line = line.trim();
    if !line.starts_with('%') {
        return None;
    }

    // Split into command + rest. We parse per-command because different
    // notifications have different field counts and quoting rules.
    let (cmd, rest) = match line.find(' ') {
        Some(pos) => (&line[..pos], line[pos + 1..].trim()),
        None => (line, ""),
    };

    match cmd {
        "%output" => parse_output(rest),
        "%extended-output" => parse_extended_output(rest),
        "%window-add" => Some(ControlModeMessage::WindowAdd {
            window_id: rest.to_string(),
        }),
        "%window-close" => Some(ControlModeMessage::WindowClose {
            window_id: rest.to_string(),
        }),
        "%window-renamed" => {
            let (window_id, new_name) = split_first_token(rest)?;
            Some(ControlModeMessage::WindowRenamed {
                window_id: window_id.to_string(),
                new_name: new_name.to_string(),
            })
        }
        "%unlinked-window-add" => Some(ControlModeMessage::UnlinkedWindowAdd {
            window_id: rest.to_string(),
        }),
        "%unlinked-window-close" => Some(ControlModeMessage::UnlinkedWindowClose {
            window_id: rest.to_string(),
        }),
        "%unlinked-window-renamed" => {
            let (window_id, new_name) = split_first_token(rest)?;
            Some(ControlModeMessage::UnlinkedWindowRenamed {
                window_id: window_id.to_string(),
                new_name: new_name.to_string(),
            })
        }
        "%session-changed" => {
            let (session_id, session_name) = split_first_token(rest)?;
            Some(ControlModeMessage::SessionChanged {
                session_id: session_id.to_string(),
                session_name: session_name.to_string(),
            })
        }
        "%session-renamed" => {
            let (session_id, new_name) = split_first_token(rest)?;
            Some(ControlModeMessage::SessionRenamed {
                session_id: session_id.to_string(),
                new_name: new_name.to_string(),
            })
        }
        "%sessions-changed" => Some(ControlModeMessage::SessionsChanged),
        "%session-window-changed" => {
            let (session_id, window_id) = split_first_token(rest)?;
            Some(ControlModeMessage::SessionWindowChanged {
                session_id: session_id.to_string(),
                window_id: window_id.to_string(),
            })
        }
        "%client-session-changed" => {
            let (client, remainder) = split_first_token(rest)?;
            let (session_id, session_name) = split_first_token(remainder)?;
            Some(ControlModeMessage::ClientSessionChanged {
                client: client.to_string(),
                session_id: session_id.to_string(),
                session_name: session_name.to_string(),
            })
        }
        "%window-pane-changed" => {
            let (window_id, pane_id) = split_first_token(rest)?;
            Some(ControlModeMessage::WindowPaneChanged {
                window_id: window_id.to_string(),
                pane_id: pane_id.to_string(),
            })
        }
        "%pane-mode-changed" => Some(ControlModeMessage::PaneModeChanged {
            pane_id: rest.to_string(),
        }),
        "%layout-change" => {
            let (window_id, layout_string) = split_first_token(rest)?;
            Some(ControlModeMessage::LayoutChange {
                window_id: window_id.to_string(),
                layout_string: layout_string.to_string(),
            })
        }
        "%pause" => Some(ControlModeMessage::Pause {
            pane_id: rest.to_string(),
        }),
        "%continue" => Some(ControlModeMessage::Continue {
            pane_id: rest.to_string(),
        }),
        "%subscription-changed" => {
            let (name, value) = split_first_token(rest)?;
            Some(ControlModeMessage::SubscriptionChanged {
                name: name.to_string(),
                value: value.to_string(),
            })
        }
        "%exit" => {
            let reason = if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            };
            Some(ControlModeMessage::Exit { reason })
        }
        _ => None,
    }
}

/// Split `s` at the first whitespace into (first_token, remainder).
fn split_first_token(s: &str) -> Option<(&str, &str)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    match s.find(' ') {
        Some(pos) => Some((&s[..pos], s[pos + 1..].trim())),
        None => Some((s, "")),
    }
}

/// Parse `%output <pane_id> <octal-data>`.
///
/// Everything after the pane_id is octal-encoded output data which may
/// contain literal spaces, so we split only on the first space.
fn parse_output(rest: &str) -> Option<ControlModeMessage> {
    let (pane_id, data_str) = split_first_token(rest)?;
    Some(ControlModeMessage::Output {
        pane_id: pane_id.to_string(),
        data: super::octal::decode_octal(data_str),
    })
}

/// Parse `%extended-output <pane_id> <lag_ms> : <octal-data>`.
fn parse_extended_output(rest: &str) -> Option<ControlModeMessage> {
    let (pane_id, remainder) = split_first_token(rest)?;
    let (lag_str, data_after_colon) = split_first_token(remainder)?;
    let lag_ms = lag_str.parse::<u64>().ok()?;
    // Skip the `:` separator if present.
    let data_str = if data_after_colon.starts_with(": ") {
        &data_after_colon[2..]
    } else if data_after_colon.starts_with(':') {
        let after = &data_after_colon[1..];
        after.trim_start()
    } else {
        data_after_colon
    };
    Some(ControlModeMessage::ExtendedOutput {
        pane_id: pane_id.to_string(),
        lag_ms,
        data: super::octal::decode_octal(data_str),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_output() {
        let msg = parse_notification("%output %0 hello\\015\\012").unwrap();
        if let ControlModeMessage::Output { pane_id, data } = msg {
            assert_eq!(pane_id, "%0");
            assert_eq!(data, b"hello\r\n");
        } else {
            panic!("Expected Output");
        }
    }

    #[test]
    fn test_parse_output_with_spaces() {
        // Octal-encoded data may contain literal spaces (ASCII >= 32).
        let msg = parse_notification("%output %5 hello world foo").unwrap();
        if let ControlModeMessage::Output { pane_id, data } = msg {
            assert_eq!(pane_id, "%5");
            assert_eq!(data, b"hello world foo");
        } else {
            panic!("Expected Output");
        }
    }

    #[test]
    fn test_parse_window_add() {
        assert_eq!(
            parse_notification("%window-add @1"),
            Some(ControlModeMessage::WindowAdd {
                window_id: "@1".into()
            })
        );
    }

    #[test]
    fn test_parse_window_close() {
        assert_eq!(
            parse_notification("%window-close @2"),
            Some(ControlModeMessage::WindowClose {
                window_id: "@2".into()
            })
        );
    }

    #[test]
    fn test_parse_window_renamed() {
        let msg = parse_notification("%window-renamed @0 my-window").unwrap();
        if let ControlModeMessage::WindowRenamed {
            window_id,
            new_name,
        } = msg
        {
            assert_eq!(window_id, "@0");
            assert_eq!(new_name, "my-window");
        } else {
            panic!("Expected WindowRenamed");
        }
    }

    #[test]
    fn test_parse_session_changed() {
        let msg = parse_notification("%session-changed $1 my-session").unwrap();
        if let ControlModeMessage::SessionChanged {
            session_id,
            session_name,
        } = msg
        {
            assert_eq!(session_id, "$1");
            assert_eq!(session_name, "my-session");
        } else {
            panic!("Expected SessionChanged");
        }
    }

    #[test]
    fn test_parse_session_renamed() {
        let msg = parse_notification("%session-renamed $0 new-name").unwrap();
        if let ControlModeMessage::SessionRenamed {
            session_id,
            new_name,
        } = msg
        {
            assert_eq!(session_id, "$0");
            assert_eq!(new_name, "new-name");
        } else {
            panic!("Expected SessionRenamed");
        }
    }

    #[test]
    fn test_parse_sessions_changed() {
        assert_eq!(
            parse_notification("%sessions-changed"),
            Some(ControlModeMessage::SessionsChanged)
        );
    }

    #[test]
    fn test_parse_session_window_changed() {
        let msg = parse_notification("%session-window-changed $0 @3").unwrap();
        if let ControlModeMessage::SessionWindowChanged {
            session_id,
            window_id,
        } = msg
        {
            assert_eq!(session_id, "$0");
            assert_eq!(window_id, "@3");
        } else {
            panic!("Expected SessionWindowChanged");
        }
    }

    #[test]
    fn test_parse_client_session_changed() {
        let msg =
            parse_notification("%client-session-changed /dev/pts/1 $2 work").unwrap();
        if let ControlModeMessage::ClientSessionChanged {
            client,
            session_id,
            session_name,
        } = msg
        {
            assert_eq!(client, "/dev/pts/1");
            assert_eq!(session_id, "$2");
            assert_eq!(session_name, "work");
        } else {
            panic!("Expected ClientSessionChanged");
        }
    }

    #[test]
    fn test_parse_window_pane_changed() {
        let msg = parse_notification("%window-pane-changed @0 %1").unwrap();
        if let ControlModeMessage::WindowPaneChanged {
            window_id,
            pane_id,
        } = msg
        {
            assert_eq!(window_id, "@0");
            assert_eq!(pane_id, "%1");
        } else {
            panic!("Expected WindowPaneChanged");
        }
    }

    #[test]
    fn test_parse_pane_mode_changed() {
        assert_eq!(
            parse_notification("%pane-mode-changed %0"),
            Some(ControlModeMessage::PaneModeChanged {
                pane_id: "%0".into()
            })
        );
    }

    #[test]
    fn test_parse_layout_change() {
        let msg = parse_notification(
            "%layout-change @0 177x44,0,0{88x44,0,0,0,88x44,89,0,1}",
        )
        .unwrap();
        if let ControlModeMessage::LayoutChange {
            window_id,
            layout_string,
        } = msg
        {
            assert_eq!(window_id, "@0");
            assert!(layout_string.contains("177x44"));
        } else {
            panic!("Expected LayoutChange");
        }
    }

    #[test]
    fn test_parse_layout_change_with_visible_layout() {
        // Some layout strings contain spaces (e.g. with window-visible-layout).
        let msg = parse_notification(
            "%layout-change @0 177x44,0,0 extra info",
        )
        .unwrap();
        if let ControlModeMessage::LayoutChange {
            window_id,
            layout_string,
        } = msg
        {
            assert_eq!(window_id, "@0");
            assert_eq!(layout_string, "177x44,0,0 extra info");
        } else {
            panic!("Expected LayoutChange");
        }
    }

    #[test]
    fn test_parse_pause() {
        assert_eq!(
            parse_notification("%pause %0"),
            Some(ControlModeMessage::Pause {
                pane_id: "%0".into()
            })
        );
    }

    #[test]
    fn test_parse_continue() {
        assert_eq!(
            parse_notification("%continue %0"),
            Some(ControlModeMessage::Continue {
                pane_id: "%0".into()
            })
        );
    }

    #[test]
    fn test_parse_subscription_changed() {
        let msg = parse_notification("%subscription-changed my_var some_value").unwrap();
        if let ControlModeMessage::SubscriptionChanged { name, value } = msg {
            assert_eq!(name, "my_var");
            assert_eq!(value, "some_value");
        } else {
            panic!("Expected SubscriptionChanged");
        }
    }

    #[test]
    fn test_parse_exit_no_reason() {
        assert_eq!(
            parse_notification("%exit"),
            Some(ControlModeMessage::Exit { reason: None })
        );
    }

    #[test]
    fn test_parse_exit_with_reason() {
        assert_eq!(
            parse_notification("%exit server exited"),
            Some(ControlModeMessage::Exit {
                reason: Some("server exited".into())
            })
        );
    }

    #[test]
    fn test_parse_unlinked_window_add() {
        assert_eq!(
            parse_notification("%unlinked-window-add @5"),
            Some(ControlModeMessage::UnlinkedWindowAdd {
                window_id: "@5".into()
            })
        );
    }

    #[test]
    fn test_parse_unlinked_window_close() {
        assert_eq!(
            parse_notification("%unlinked-window-close @5"),
            Some(ControlModeMessage::UnlinkedWindowClose {
                window_id: "@5".into()
            })
        );
    }

    #[test]
    fn test_parse_unlinked_window_renamed() {
        let msg = parse_notification("%unlinked-window-renamed @3 new-name").unwrap();
        if let ControlModeMessage::UnlinkedWindowRenamed {
            window_id,
            new_name,
        } = msg
        {
            assert_eq!(window_id, "@3");
            assert_eq!(new_name, "new-name");
        } else {
            panic!("Expected UnlinkedWindowRenamed");
        }
    }

    #[test]
    fn test_parse_unknown() {
        assert_eq!(parse_notification("%unknown-event foo"), None);
    }

    #[test]
    fn test_parse_empty() {
        assert_eq!(parse_notification(""), None);
    }

    #[test]
    fn test_parse_non_notification() {
        assert_eq!(parse_notification("just some text"), None);
    }

    #[test]
    fn test_parse_whitespace_only() {
        assert_eq!(parse_notification("   "), None);
    }
}
