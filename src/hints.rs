//! Hint-mode overlay: scan terminal text for URLs, file paths, and git hashes,
//! then assign short keyboard labels so the user can jump/copy with a few
//! keystrokes (similar to tmux-fingers / vimium).

use regex::Regex;
use std::collections::BTreeSet;

// ---------------------------------------------------------------------------
// Pattern constants
// ---------------------------------------------------------------------------

/// Matches `http://` and `https://` URLs up to whitespace or closing brackets.
pub const URL_PATTERN: &str = r"https?://[^\s\)\]>]+";

/// Matches file paths that contain at least one forward slash and a dot
/// extension (e.g. `src/main.rs:42`).
pub const PATH_PATTERN: &str = r"[A-Za-z0-9_\-\.]+/[A-Za-z0-9_\-\./]+\.\w+(:\d+)?";

/// Matches 7-40 character lowercase hex strings as whole words.
pub const HASH_PATTERN: &str = r"\b[0-9a-f]{7,40}\b";

/// Default hint keys used to build labels.
pub const DEFAULT_HINT_KEYS: &str = "asdfjkl;";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single detected match together with its keyboard label.
#[derive(Clone, Debug)]
pub struct HintMatch {
    pub row: u16,
    pub start_col: u16,
    pub end_col: u16,
    pub text: String,
    pub label: String,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Scan `lines` for URLs, file paths and git hashes, deduplicate, sort
/// top-left to bottom-right, and assign keyboard labels.
///
/// The maximum number of labelled hints is `n + n*n` where
/// `n = hint_keys.len()`.
pub fn scan_and_label(lines: &[String], hint_keys: &str) -> Vec<HintMatch> {
    let keys: Vec<char> = hint_keys.chars().collect();
    if keys.is_empty() {
        return Vec::new();
    }

    let url_re = Regex::new(URL_PATTERN).expect("invalid URL_PATTERN");
    let path_re = Regex::new(PATH_PATTERN).expect("invalid PATH_PATTERN");
    let hash_re = Regex::new(HASH_PATTERN).expect("invalid HASH_PATTERN");

    // Collect raw matches – use a BTreeSet of (row, start_col) for dedup.
    let mut seen: BTreeSet<(u16, u16)> = BTreeSet::new();
    let mut raw: Vec<HintMatch> = Vec::new();

    for (row_idx, line) in lines.iter().enumerate() {
        let row = row_idx as u16;
        for re in [&url_re, &path_re, &hash_re] {
            for m in re.find_iter(line) {
                let start_col = m.start() as u16;
                let end_col = m.end() as u16;
                if seen.insert((row, start_col)) {
                    raw.push(HintMatch {
                        row,
                        start_col,
                        end_col,
                        text: m.as_str().to_string(),
                        label: String::new(), // assigned below
                    });
                }
            }
        }
    }

    // Sort top-left to bottom-right (row first, then column).
    raw.sort_by(|a, b| a.row.cmp(&b.row).then(a.start_col.cmp(&b.start_col)));

    // Cap total at n + n*n.
    let n = keys.len();
    let max_labels = n + n * n;
    raw.truncate(max_labels);

    // Assign labels.
    for (idx, hint) in raw.iter_mut().enumerate() {
        hint.label = make_label(idx, &keys);
    }

    raw
}

/// Build a keyboard label for the given index.
///
/// * `idx < n`  -> single character (`keys[idx]`)
/// * `idx >= n` -> two characters (`keys[(idx-n)/n]`, `keys[(idx-n)%n]`)
pub fn make_label(idx: usize, keys: &[char]) -> String {
    let n = keys.len();
    if idx < n {
        keys[idx].to_string()
    } else {
        let adjusted = idx - n;
        let first = keys[adjusted / n];
        let second = keys[adjusted % n];
        format!("{}{}", first, second)
    }
}

/// Find the first match whose label equals `input` exactly.
pub fn find_match<'a>(matches: &'a [HintMatch], input: &str) -> Option<&'a HintMatch> {
    matches.iter().find(|m| m.label == input)
}

/// Returns `true` if any label starts with `input`.
pub fn has_prefix(matches: &[HintMatch], input: &str) -> bool {
    matches.iter().any(|m| m.label.starts_with(input))
}

// ---------------------------------------------------------------------------
// Enter hints mode from AppState
// ---------------------------------------------------------------------------

use crate::types::{AppState, HintsState, Mode, Node, Pane};

/// Walk the split tree to find the active pane.
fn find_active_pane<'a>(node: &'a Node, path: &[usize]) -> Option<&'a Pane> {
    match node {
        Node::Leaf(p) => Some(p),
        Node::Split { children, .. } => {
            let idx = path.first().copied().unwrap_or(0);
            children
                .get(idx)
                .and_then(|child| find_active_pane(child, &path[1..]))
        }
    }
}

/// Extract visible text lines from the active pane's terminal screen.
fn extract_visible_lines(app: &AppState) -> Vec<String> {
    let win = match app.windows.get(app.active_idx) {
        Some(w) => w,
        None => return Vec::new(),
    };
    let pane = match find_active_pane(&win.root, &win.active_path) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let parser = pane.term.lock().unwrap();
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let mut lines = Vec::new();
    for row in 0..rows {
        let mut line = String::new();
        for col in 0..cols {
            if let Some(cell) = screen.cell(row, col) {
                let contents: &str = cell.contents();
                if contents.is_empty() {
                    line.push(' ');
                } else {
                    line.push_str(contents);
                }
            }
        }
        lines.push(line.trim_end().to_string());
    }
    lines
}

/// Enter hints mode: scan the active pane's visible output for patterns.
/// If no matches are found, the mode is not changed.
pub fn enter_hints_mode(app: &mut AppState) {
    let hint_keys = app.hint_keys.clone();
    let lines = extract_visible_lines(app);
    let matches = scan_and_label(&lines, &hint_keys);
    if matches.is_empty() {
        return;
    }
    app.mode = Mode::HintsMode(Box::new(HintsState {
        matches,
        input: String::new(),
        entered_at: std::time::Instant::now(),
    }));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_urls() {
        let lines = vec!["Visit https://github.com/foo/bar for info".to_string()];
        let matches = scan_and_label(&lines, DEFAULT_HINT_KEYS);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].text, "https://github.com/foo/bar");
    }

    #[test]
    fn scan_file_paths() {
        let lines = vec!["edit src/main.rs:42 now".to_string()];
        let matches = scan_and_label(&lines, DEFAULT_HINT_KEYS);
        assert!(
            matches.iter().any(|m| m.text == "src/main.rs:42"),
            "expected src/main.rs:42 among matches: {:?}",
            matches
        );
    }

    #[test]
    fn scan_git_hashes() {
        let lines = vec!["commit 8614151 feat: something".to_string()];
        let matches = scan_and_label(&lines, DEFAULT_HINT_KEYS);
        assert!(
            matches.iter().any(|m| m.text == "8614151"),
            "expected 8614151 among matches: {:?}",
            matches
        );
    }

    #[test]
    fn labels_unique() {
        // 10+ URLs on one line to exercise multi-char labels.
        let urls: Vec<String> = (0..12)
            .map(|i| format!("https://example.com/{}", i))
            .collect();
        let line = urls.join(" ");
        let lines = vec![line];
        let matches = scan_and_label(&lines, DEFAULT_HINT_KEYS);
        assert!(matches.len() >= 10, "expected >=10 matches");

        let mut labels: Vec<&str> = matches.iter().map(|m| m.label.as_str()).collect();
        let count_before = labels.len();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), count_before, "all labels must be unique");
    }

    #[test]
    fn find_match_exact() {
        let lines = vec!["see https://example.com ok".to_string()];
        let matches = scan_and_label(&lines, DEFAULT_HINT_KEYS);
        assert!(!matches.is_empty());
        let first_label = matches[0].label.clone();
        assert_eq!(first_label, "a");
        let found = find_match(&matches, "a");
        assert!(found.is_some());
        assert_eq!(found.unwrap().text, "https://example.com");
    }

    #[test]
    fn has_prefix_partial() {
        let lines = vec!["see https://example.com ok".to_string()];
        let matches = scan_and_label(&lines, DEFAULT_HINT_KEYS);
        assert!(has_prefix(&matches, "a"), "prefix 'a' should exist");
        assert!(!has_prefix(&matches, "z"), "prefix 'z' should not exist");
    }
}
