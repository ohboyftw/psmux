//! Remote pane manager — maps tmux control mode events to VT100 parsers.
//!
//! [`RemotePaneManager`] maintains a collection of [`vt100::Parser`] instances,
//! one per remote tmux pane.  When the [`ControlModeParser`](super::parser)
//! produces a [`ControlModeMessage`], it is fed to
//! [`RemotePaneManager::handle_message`] which routes output data to the
//! correct parser and tracks window/pane membership.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::protocol::ControlModeMessage;

/// Manages VT100 parser state for every pane in a remote tmux session.
///
/// The manager is the bridge between the structured control mode event stream
/// and the per-pane terminal emulation that psmux's rendering layer expects.
pub struct RemotePaneManager {
    /// pane_id (e.g. "%0") -> VT100 parser for rendering
    panes: HashMap<String, Arc<Mutex<vt100::Parser>>>,
    /// window_id (e.g. "@0") -> ordered list of pane_ids
    windows: HashMap<String, Vec<String>>,
    /// Currently active pane id
    active_pane: Option<String>,
    /// Terminal columns
    cols: u16,
    /// Terminal rows
    rows: u16,
}

impl RemotePaneManager {
    /// Create a new manager with the given terminal dimensions.
    ///
    /// New panes created via [`ControlModeMessage::LayoutChange`] will be
    /// initialised with these dimensions.
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            panes: HashMap::new(),
            windows: HashMap::new(),
            active_pane: None,
            cols,
            rows,
        }
    }

    /// Process a control mode message, updating internal state.
    ///
    /// This is the main entry point — call it for every message produced by
    /// [`ControlModeParser`](super::parser::ControlModeParser).
    pub fn handle_message(&mut self, msg: ControlModeMessage) {
        match msg {
            ControlModeMessage::Output { pane_id, data } => {
                if let Some(parser) = self.panes.get(&pane_id) {
                    if let Ok(mut p) = parser.lock() {
                        p.process(&data);
                    }
                }
            }
            ControlModeMessage::ExtendedOutput { pane_id, data, .. } => {
                // Treat the same as Output — the lag_ms field is informational.
                if let Some(parser) = self.panes.get(&pane_id) {
                    if let Ok(mut p) = parser.lock() {
                        p.process(&data);
                    }
                }
            }
            ControlModeMessage::WindowAdd { window_id } => {
                self.windows.entry(window_id).or_default();
            }
            ControlModeMessage::WindowClose { window_id } => {
                if let Some(pane_ids) = self.windows.remove(&window_id) {
                    for pid in pane_ids {
                        self.panes.remove(&pid);
                        // Clear active pane if it belonged to this window.
                        if self.active_pane.as_deref() == Some(&pid) {
                            self.active_pane = None;
                        }
                    }
                }
            }
            ControlModeMessage::LayoutChange {
                window_id,
                layout_string,
            } => {
                let pane_ids = parse_layout_pane_ids(&layout_string);
                for pid in &pane_ids {
                    if !self.panes.contains_key(pid) {
                        self.panes.insert(
                            pid.clone(),
                            Arc::new(Mutex::new(vt100::Parser::new(self.rows, self.cols, 0))),
                        );
                    }
                }
                self.windows.insert(window_id, pane_ids);
            }
            ControlModeMessage::WindowPaneChanged { pane_id, .. } => {
                self.active_pane = Some(pane_id);
            }
            ControlModeMessage::Exit { .. } => {
                // Session ended — will be handled by SshTransport (Task 14).
            }
            _ => {
                // Other notifications (SessionChanged, WindowRenamed, etc.)
                // are not relevant to pane state — ignore for now.
            }
        }
    }

    /// Get the VT100 parser for the currently active pane, if any.
    pub fn get_active_pane(&self) -> Option<&Arc<Mutex<vt100::Parser>>> {
        self.active_pane.as_ref().and_then(|id| self.panes.get(id))
    }

    /// Get the VT100 parser for a specific pane by ID.
    pub fn get_pane(&self, pane_id: &str) -> Option<&Arc<Mutex<vt100::Parser>>> {
        self.panes.get(pane_id)
    }

    /// Return all tracked pane IDs.
    pub fn pane_ids(&self) -> Vec<String> {
        self.panes.keys().cloned().collect()
    }

    /// Return the ID of the currently active pane.
    pub fn active_pane_id(&self) -> Option<&str> {
        self.active_pane.as_deref()
    }

    /// Return the window -> pane-list mapping.
    pub fn windows(&self) -> &HashMap<String, Vec<String>> {
        &self.windows
    }
}

/// Extract pane IDs from a tmux layout string.
///
/// tmux layout strings describe the pane geometry recursively. Leaf nodes
/// (individual panes) have the form `WxH,X,Y,PANE_ID` where PANE_ID is a
/// plain integer.  Branch nodes use `{...}` for horizontal splits and
/// `[...]` for vertical splits; they look like `WxH,X,Y{child,child,...}`
/// or `WxH,X,Y[child,child,...]` (no fourth comma-delimited integer).
///
/// # Examples
///
/// Single pane: `"80x24,0,0,0"` -> `["%0"]`
///
/// Two horizontal panes: `"80x24,0,0{40x24,0,0,0,39x24,41,0,1}"` -> `["%0", "%1"]`
fn parse_layout_pane_ids(layout: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let bytes = layout.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Look for the start of a geometry spec: one or more digits followed by 'x'.
        if bytes[i].is_ascii_digit() {
            // Skip first number (width)
            while i < len && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < len && bytes[i] == b'x' {
                i += 1;
                // Skip second number (height)
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                // Now we expect: ,X,Y,PANE_ID  (leaf) or ,X,Y{... / ,X,Y[... (branch)
                // Count commas and capture potential pane ID.
                let mut comma_count = 0;
                let mut pane_id_start = 0;
                let mut pane_id_end = 0;
                let mut j = i;
                let mut is_leaf = true;

                while j < len && comma_count < 3 {
                    if bytes[j] == b',' {
                        comma_count += 1;
                        j += 1;
                        if comma_count == 3 {
                            // After the third comma: check if it's digits (leaf)
                            // or a brace/bracket (branch).
                            if j < len && bytes[j].is_ascii_digit() {
                                pane_id_start = j;
                                while j < len && bytes[j].is_ascii_digit() {
                                    j += 1;
                                }
                                pane_id_end = j;
                            } else {
                                is_leaf = false;
                            }
                        }
                    } else if bytes[j] == b'{' || bytes[j] == b'[' {
                        // Branch node — not a leaf, stop scanning commas.
                        is_leaf = false;
                        break;
                    } else {
                        j += 1;
                    }
                }

                if is_leaf && comma_count == 3 && pane_id_end > pane_id_start {
                    // Validate that the character after the pane ID is not
                    // another digit (paranoia check).
                    let ok = pane_id_end >= len || !bytes[pane_id_end].is_ascii_digit();
                    if ok {
                        let id_str = &layout[pane_id_start..pane_id_end];
                        let pane_id = format!("%{id_str}");
                        if !ids.contains(&pane_id) {
                            ids.push(pane_id);
                        }
                    }
                }
                i = j;
            }
            // If we didn't see 'x', just continue — i was already advanced
            // past the digits.
        } else {
            i += 1;
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_layout_pane_ids ──────────────────────────────────────────

    #[test]
    fn test_parse_layout_single_pane() {
        assert_eq!(parse_layout_pane_ids("80x24,0,0,0"), vec!["%0"]);
    }

    #[test]
    fn test_parse_layout_two_panes_horizontal() {
        let ids = parse_layout_pane_ids("80x24,0,0{40x24,0,0,0,39x24,41,0,1}");
        assert_eq!(ids, vec!["%0", "%1"]);
    }

    #[test]
    fn test_parse_layout_three_panes_mixed() {
        let ids =
            parse_layout_pane_ids("177x44,0,0{88x44,0,0,0,88x44,89,0[88x22,89,0,1,88x21,89,23,2]}");
        assert_eq!(ids, vec!["%0", "%1", "%2"]);
    }

    #[test]
    fn test_parse_layout_no_phantom_ids() {
        // The geometry numbers (80, 24, 0) must NOT appear as pane IDs.
        let ids = parse_layout_pane_ids("80x24,0,0,5");
        assert_eq!(ids, vec!["%5"]);
        assert!(!ids.contains(&"%80".to_string()));
        assert!(!ids.contains(&"%24".to_string()));
    }

    #[test]
    fn test_parse_layout_empty() {
        assert!(parse_layout_pane_ids("").is_empty());
    }

    #[test]
    fn test_parse_layout_large_pane_ids() {
        let ids = parse_layout_pane_ids("80x24,0,0{40x24,0,0,42,39x24,41,0,137}");
        assert_eq!(ids, vec!["%42", "%137"]);
    }

    // ── RemotePaneManager ─────────────────────────────────────────────

    #[test]
    fn test_handle_output_routes_to_pane() {
        let mut mgr = RemotePaneManager::new(80, 24);
        mgr.panes.insert(
            "%0".into(),
            Arc::new(Mutex::new(vt100::Parser::new(24, 80, 0))),
        );

        mgr.handle_message(ControlModeMessage::Output {
            pane_id: "%0".into(),
            data: b"hello".to_vec(),
        });

        let parser = mgr.panes.get("%0").unwrap().lock().unwrap();
        let contents = parser.screen().contents();
        assert!(contents.contains("hello"));
    }

    #[test]
    fn test_handle_extended_output_routes_to_pane() {
        let mut mgr = RemotePaneManager::new(80, 24);
        mgr.panes.insert(
            "%0".into(),
            Arc::new(Mutex::new(vt100::Parser::new(24, 80, 0))),
        );

        mgr.handle_message(ControlModeMessage::ExtendedOutput {
            pane_id: "%0".into(),
            lag_ms: 10,
            data: b"world".to_vec(),
        });

        let parser = mgr.panes.get("%0").unwrap().lock().unwrap();
        let contents = parser.screen().contents();
        assert!(contents.contains("world"));
    }

    #[test]
    fn test_output_to_unknown_pane_is_noop() {
        let mut mgr = RemotePaneManager::new(80, 24);
        // No panic, no error — just silently ignored.
        mgr.handle_message(ControlModeMessage::Output {
            pane_id: "%99".into(),
            data: b"stray".to_vec(),
        });
        assert!(mgr.panes.is_empty());
    }

    #[test]
    fn test_window_add_close() {
        let mut mgr = RemotePaneManager::new(80, 24);

        mgr.handle_message(ControlModeMessage::WindowAdd {
            window_id: "@0".into(),
        });
        assert!(mgr.windows.contains_key("@0"));

        mgr.handle_message(ControlModeMessage::WindowClose {
            window_id: "@0".into(),
        });
        assert!(!mgr.windows.contains_key("@0"));
    }

    #[test]
    fn test_window_close_removes_panes() {
        let mut mgr = RemotePaneManager::new(80, 24);

        // Simulate a layout that creates panes in window @0.
        mgr.handle_message(ControlModeMessage::LayoutChange {
            window_id: "@0".into(),
            layout_string: "80x24,0,0{40x24,0,0,0,39x24,41,0,1}".into(),
        });
        assert_eq!(mgr.panes.len(), 2);

        // Set active pane to one of the panes in the window.
        mgr.handle_message(ControlModeMessage::WindowPaneChanged {
            window_id: "@0".into(),
            pane_id: "%0".into(),
        });
        assert_eq!(mgr.active_pane_id(), Some("%0"));

        // Closing the window removes panes and clears active.
        mgr.handle_message(ControlModeMessage::WindowClose {
            window_id: "@0".into(),
        });
        assert!(mgr.panes.is_empty());
        assert!(mgr.active_pane_id().is_none());
    }

    #[test]
    fn test_layout_change_creates_panes() {
        let mut mgr = RemotePaneManager::new(80, 24);

        mgr.handle_message(ControlModeMessage::LayoutChange {
            window_id: "@0".into(),
            layout_string: "80x24,0,0{40x24,0,0,0,39x24,41,0,1}".into(),
        });
        assert!(mgr.panes.contains_key("%0"));
        assert!(mgr.panes.contains_key("%1"));
        assert_eq!(mgr.panes.len(), 2);
        assert_eq!(
            mgr.windows.get("@0").unwrap(),
            &vec!["%0".to_string(), "%1".to_string()]
        );
    }

    #[test]
    fn test_layout_change_preserves_existing_parsers() {
        let mut mgr = RemotePaneManager::new(80, 24);

        // First layout creates pane %0.
        mgr.handle_message(ControlModeMessage::LayoutChange {
            window_id: "@0".into(),
            layout_string: "80x24,0,0,0".into(),
        });
        let ptr_before = Arc::as_ptr(mgr.panes.get("%0").unwrap());

        // Feed some output so the parser has state.
        mgr.handle_message(ControlModeMessage::Output {
            pane_id: "%0".into(),
            data: b"keep me".to_vec(),
        });

        // Second layout adds pane %1 but %0 still exists.
        mgr.handle_message(ControlModeMessage::LayoutChange {
            window_id: "@0".into(),
            layout_string: "80x24,0,0{40x24,0,0,0,39x24,41,0,1}".into(),
        });
        let ptr_after = Arc::as_ptr(mgr.panes.get("%0").unwrap());

        // Same Arc — parser was NOT recreated.
        assert_eq!(ptr_before, ptr_after);

        // Verify the original content is still there.
        let parser = mgr.panes.get("%0").unwrap().lock().unwrap();
        assert!(parser.screen().contents().contains("keep me"));
    }

    #[test]
    fn test_active_pane_tracking() {
        let mut mgr = RemotePaneManager::new(80, 24);
        mgr.panes.insert(
            "%0".into(),
            Arc::new(Mutex::new(vt100::Parser::new(24, 80, 0))),
        );
        mgr.panes.insert(
            "%1".into(),
            Arc::new(Mutex::new(vt100::Parser::new(24, 80, 0))),
        );

        assert!(mgr.active_pane_id().is_none());
        assert!(mgr.get_active_pane().is_none());

        mgr.handle_message(ControlModeMessage::WindowPaneChanged {
            window_id: "@0".into(),
            pane_id: "%1".into(),
        });
        assert_eq!(mgr.active_pane_id(), Some("%1"));
        assert!(mgr.get_active_pane().is_some());
    }

    #[test]
    fn test_pane_ids_returns_all() {
        let mut mgr = RemotePaneManager::new(80, 24);
        mgr.handle_message(ControlModeMessage::LayoutChange {
            window_id: "@0".into(),
            layout_string: "177x44,0,0{88x44,0,0,0,88x44,89,0[88x22,89,0,1,88x21,89,23,2]}".into(),
        });
        let mut ids = mgr.pane_ids();
        ids.sort();
        assert_eq!(ids, vec!["%0", "%1", "%2"]);
    }

    #[test]
    fn test_exit_is_noop() {
        let mut mgr = RemotePaneManager::new(80, 24);
        mgr.panes.insert(
            "%0".into(),
            Arc::new(Mutex::new(vt100::Parser::new(24, 80, 0))),
        );
        // Exit doesn't clear state — the caller (SshTransport) handles teardown.
        mgr.handle_message(ControlModeMessage::Exit {
            reason: Some("server exited".into()),
        });
        assert_eq!(mgr.panes.len(), 1);
    }
}
