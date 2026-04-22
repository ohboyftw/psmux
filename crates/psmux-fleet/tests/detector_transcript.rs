#![cfg(feature = "fleet-claude-transcript")]

use psmux_fleet::detector::ClaudeTranscriptDetector;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

#[test]
fn unmatched_tool_use_returns_reason() {
    let d = ClaudeTranscriptDetector::new();
    let reason = d
        .boost_reason(&fixture("transcript_awaiting.jsonl"))
        .unwrap();
    assert_eq!(reason, "Tool approval: Edit");
}

#[test]
fn matched_tool_use_returns_none() {
    let d = ClaudeTranscriptDetector::new();
    let reason = d.boost_reason(&fixture("transcript_idle.jsonl"));
    assert!(reason.is_none());
}

#[test]
fn malformed_jsonl_returns_none_not_error() {
    let d = ClaudeTranscriptDetector::new();
    let tmp_path = std::env::temp_dir().join(format!(
        "fleet-transcript-malformed-{}.jsonl",
        std::process::id()
    ));
    std::fs::write(&tmp_path, "not valid json at all\n{\"also\":\"broken\"").unwrap();
    // Should silently downgrade — regex verdict stands.
    assert!(d.boost_reason(&tmp_path).is_none());
    let _ = std::fs::remove_file(&tmp_path);
}

#[test]
fn missing_file_returns_none_not_error() {
    let d = ClaudeTranscriptDetector::new();
    let nonexistent = PathBuf::from("D:/nonexistent/path/transcript.jsonl");
    assert!(d.boost_reason(&nonexistent).is_none());
}
