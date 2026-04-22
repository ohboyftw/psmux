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

#[cfg(feature = "fleet-claude-transcript")]
pub use transcript::ClaudeTranscriptDetector;

#[cfg(feature = "fleet-claude-transcript")]
mod transcript {
    use std::collections::HashSet;
    use std::path::Path;

    /// Booster that parses Claude's private JSONL transcript format
    /// (`~/.claude/projects/<proj>/<session>.jsonl`) to produce a precise
    /// `awaiting_reason` string.
    ///
    /// # Contract
    ///
    /// This is a **refinement only** path (spec §8.2). It never flips a
    /// workspace to `Awaiting` on its own — the `RegexDetector` holds that
    /// responsibility. When this returns `None` (parse failure, missing
    /// file, all tool_use entries matched), the regex verdict stands.
    ///
    /// # Stability
    ///
    /// Claude Code's JSONL format is undocumented and subject to change.
    /// Any parse error silently downgrades to `None` rather than
    /// propagating — a broken Claude release degrades `awaiting_reason`
    /// quality but cannot take FleetView down.
    pub struct ClaudeTranscriptDetector;

    impl ClaudeTranscriptDetector {
        /// Constructs a new detector. Stateless — no configuration required.
        pub fn new() -> Self {
            Self
        }

        /// Parses the transcript at `path`. Returns `Some("Tool approval: <name>")`
        /// iff the last `tool_use` event has no matching `tool_result`.
        /// Silent `None` on any parse or I/O failure.
        pub fn boost_reason(&self, path: &Path) -> Option<String> {
            let text = std::fs::read_to_string(path).ok()?;
            let mut last_tool_use: Option<(String, String)> = None; // (id, name)
            let mut matched: HashSet<String> = HashSet::new();
            for line in text.lines() {
                let v: serde_json::Value = match serde_json::from_str(line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(tu) = v.get("tool_use") {
                    if let (Some(id), Some(name)) = (
                        tu.get("id").and_then(|s| s.as_str()),
                        tu.get("name").and_then(|s| s.as_str()),
                    ) {
                        last_tool_use = Some((id.to_string(), name.to_string()));
                    }
                }
                if v.get("type").and_then(|s| s.as_str()) == Some("tool_result") {
                    if let Some(id) = v.get("tool_use_id").and_then(|s| s.as_str()) {
                        matched.insert(id.to_string());
                    }
                }
            }
            let (id, name) = last_tool_use?;
            if matched.contains(&id) {
                None
            } else {
                Some(format!("Tool approval: {name}"))
            }
        }
    }

    impl Default for ClaudeTranscriptDetector {
        fn default() -> Self {
            Self::new()
        }
    }
}
