//! Awaiting-input detection.
//!
//! Regex matching against the last 10 lines of pane output is the primary
//! detection path (spec §8.1). T8 adds a Claude JSONL transcript booster
//! that refines `awaiting_reason` when available but does not flip state
//! on its own.

use crate::AgentKind;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 2 Hz debounce per workspace — at most one evaluation every 500ms.
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(500);

/// Strategy for detecting whether an agent is awaiting input.
pub trait AwaitingDetector: Send + Sync {
    /// Returns the matched-pattern reason if the tail of `text` indicates
    /// the agent is awaiting input. `None` otherwise.
    fn detect(&self, kind: AgentKind, text: &str) -> Option<String>;
}

/// Regex-based detector seeded with OMC's production patterns.
///
/// Per-workspace 2 Hz debounce via [`Self::try_detect`]. Case-sensitive,
/// word-boundary anchored where appropriate. The word "autonomous" in
/// prose must not trigger a match (OMC `d6354c8` regression guard).
pub struct RegexDetector {
    patterns: HashMap<AgentKind, Vec<Regex>>,
    last_eval: Mutex<HashMap<String, Instant>>,
}

impl RegexDetector {
    /// Constructs a detector with the default pattern registry.
    pub fn new_with_defaults() -> Self {
        let mut patterns: HashMap<AgentKind, Vec<Regex>> = HashMap::new();
        patterns.insert(
            AgentKind::Claude,
            vec![
                Regex::new(r"Do you want to proceed\?").unwrap(),
                Regex::new(r"\[y/n\]").unwrap(),
                Regex::new(r"\(y/N\)").unwrap(),
                Regex::new(r"Press Enter to continue").unwrap(),
            ],
        );
        patterns.insert(
            AgentKind::Shell,
            vec![
                Regex::new(r"\$ $").unwrap(),
                Regex::new(r"PS .*> $").unwrap(),
                Regex::new(r"^> $").unwrap(),
            ],
        );
        Self {
            patterns,
            last_eval: Mutex::new(HashMap::new()),
        }
    }

    /// Per-workspace debounced detect.
    ///
    /// Returns `true` iff the detector was allowed to evaluate during this
    /// window AND found a match. Returns `false` if debounced or if no
    /// match was found.
    pub fn try_detect(&self, workspace_id: &str, kind: AgentKind, text: &str) -> bool {
        let now = Instant::now();
        let mut last = self.last_eval.lock().unwrap();
        if let Some(prev) = last.get(workspace_id) {
            if now.duration_since(*prev) < DEBOUNCE_WINDOW {
                return false;
            }
        }
        last.insert(workspace_id.to_string(), now);
        drop(last);
        self.detect(kind, text).is_some()
    }
}

impl AwaitingDetector for RegexDetector {
    fn detect(&self, kind: AgentKind, text: &str) -> Option<String> {
        // Match against the last 10 lines only (spec §8.1).
        let last_lines: Vec<&str> = text.lines().collect();
        let start = last_lines.len().saturating_sub(10);
        let tail = last_lines[start..].join("\n");
        let patterns = self.patterns.get(&kind)?;
        for re in patterns {
            if let Some(m) = re.find(&tail) {
                return Some(m.as_str().to_string());
            }
        }
        None
    }
}
