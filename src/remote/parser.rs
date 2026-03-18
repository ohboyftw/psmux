//! Control mode parser state machine.
//!
//! tmux control mode output is a mix of:
//!  - **Response blocks**: `%begin <ts> <id> <flags>` … body lines … `%end`/`%error`
//!  - **Standalone notifications**: single lines starting with `%` (e.g. `%output`)
//!
//! The [`ControlModeParser`] accepts one line at a time via [`feed_line`] and
//! returns a [`ControlModeMessage`] whenever a complete message has been
//! assembled.
//!
//! [`feed_line`]: ControlModeParser::feed_line

use super::protocol::{parse_notification, ControlModeMessage};

/// State machine for parsing tmux control mode output line-by-line.
pub struct ControlModeParser {
    state: ParserState,
    block_lines: Vec<String>,
    block_timestamp: u64,
    block_command_id: u64,
    block_flags: u32,
}

/// Internal parser state.
enum ParserState {
    /// Waiting for a `%begin` or a standalone notification.
    Idle,
    /// Inside a `%begin` … `%end`/`%error` response block.
    InBlock,
}

impl ControlModeParser {
    /// Create a new parser in the idle state.
    pub fn new() -> Self {
        Self {
            state: ParserState::Idle,
            block_lines: Vec::new(),
            block_timestamp: 0,
            block_command_id: 0,
            block_flags: 0,
        }
    }

    /// Feed a single line of control mode output.
    ///
    /// Returns a parsed [`ControlModeMessage`] when a complete message has been
    /// assembled (either a standalone notification or a fully-delimited response
    /// block). Returns `None` for partial blocks and unrecognised lines.
    pub fn feed_line(&mut self, line: &str) -> Option<ControlModeMessage> {
        let line = line.trim_end();

        match self.state {
            ParserState::Idle => {
                if line.starts_with("%begin ") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 4 {
                        self.block_timestamp = parts[1].parse().unwrap_or(0);
                        self.block_command_id = parts[2].parse().unwrap_or(0);
                        self.block_flags = parts[3].parse().unwrap_or(0);
                        self.block_lines.clear();
                        self.state = ParserState::InBlock;
                    }
                    None
                } else {
                    parse_notification(line)
                }
            }
            ParserState::InBlock => {
                if line.starts_with("%end ") || line.starts_with("%error ") {
                    let success = line.starts_with("%end");
                    let msg = ControlModeMessage::Response {
                        timestamp: self.block_timestamp,
                        command_id: self.block_command_id,
                        flags: self.block_flags,
                        body: std::mem::take(&mut self.block_lines),
                        success,
                    };
                    self.state = ParserState::Idle;
                    Some(msg)
                } else {
                    self.block_lines.push(line.to_string());
                    None
                }
            }
        }
    }
}

impl Default for ControlModeParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_block() {
        let mut parser = ControlModeParser::new();
        assert!(parser.feed_line("%begin 1711234567 42 0").is_none());
        assert!(parser.feed_line("%3").is_none());
        let msg = parser.feed_line("%end 1711234567 42 0").unwrap();
        if let ControlModeMessage::Response {
            command_id,
            body,
            success,
            ..
        } = msg
        {
            assert_eq!(command_id, 42);
            assert!(success);
            assert_eq!(body, vec!["%3"]);
        } else {
            panic!("Expected Response");
        }
    }

    #[test]
    fn test_multiline_response() {
        let mut parser = ControlModeParser::new();
        parser.feed_line("%begin 0 1 0");
        parser.feed_line("0: window-name (1 panes)");
        parser.feed_line("1: vim (1 panes)");
        let msg = parser.feed_line("%end 0 1 0").unwrap();
        if let ControlModeMessage::Response { body, .. } = msg {
            assert_eq!(body.len(), 2);
        } else {
            panic!("Expected Response");
        }
    }

    #[test]
    fn test_error_response() {
        let mut parser = ControlModeParser::new();
        parser.feed_line("%begin 0 5 0");
        parser.feed_line("no such session");
        let msg = parser.feed_line("%error 0 5 0").unwrap();
        if let ControlModeMessage::Response { success, body, .. } = msg {
            assert!(!success);
            assert_eq!(body[0], "no such session");
        } else {
            panic!("Expected Response");
        }
    }

    #[test]
    fn test_interleaved_notifications_and_blocks() {
        let mut parser = ControlModeParser::new();
        let n = parser.feed_line("%output %0 hello").unwrap();
        assert!(matches!(n, ControlModeMessage::Output { .. }));

        assert!(parser.feed_line("%begin 0 1 0").is_none());
        assert!(parser.feed_line("response line").is_none());
        let r = parser.feed_line("%end 0 1 0").unwrap();
        assert!(matches!(r, ControlModeMessage::Response { .. }));

        let n2 = parser.feed_line("%window-add @1").unwrap();
        assert!(matches!(n2, ControlModeMessage::WindowAdd { .. }));
    }

    #[test]
    fn test_empty_response_block() {
        let mut parser = ControlModeParser::new();
        parser.feed_line("%begin 0 0 0");
        let msg = parser.feed_line("%end 0 0 0").unwrap();
        if let ControlModeMessage::Response { body, success, .. } = msg {
            assert!(success);
            assert!(body.is_empty());
        } else {
            panic!("Expected Response");
        }
    }

    #[test]
    fn test_default_trait() {
        let parser = ControlModeParser::default();
        // Just verify it compiles and doesn't panic.
        drop(parser);
    }
}
