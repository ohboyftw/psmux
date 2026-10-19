// Topic validation and matching tests for src/bus/topic.rs.
// Covers mycel spec 3.3 and 12.6 #1-#3 under the fixed (MQTT) rules.

use super::*;

const PSMUX_TOPICS: &[&str] = &[
    "psmux/pane/created",
    "psmux/pane/ready",
    "psmux/pane/exited",
    "psmux/pane/died",
    "psmux/exec/completed",
    "psmux/session/created",
    "psmux/session/renamed",
    "psmux/session/killed",
];

const SYSTEM_TOPICS: &[&str] = &[
    "$mycel/server/started",
    "$mycel/clients/connected",
    "$mycel/clients/disconnected",
    "$mycel/clients/heartbeat",
    "$mycel/server/buffer_overflow",
];

#[test]
fn every_known_topic_is_valid() {
    for t in PSMUX_TOPICS.iter().chain(SYSTEM_TOPICS) {
        assert_eq!(validate_topic(t), Ok(*t), "{t}");
    }
}

#[test]
fn empty_topic_is_rejected() {
    assert_eq!(validate_topic(""), Err(TopicError::Empty));
    assert_eq!(validate_pattern(""), Err(TopicError::Empty));
}

#[test]
fn topic_longer_than_limit_is_rejected() {
    let long = "a".repeat(MAX_TOPIC_LEN + 1);
    assert_eq!(validate_topic(&long), Err(TopicError::TooLong));
    assert_eq!(validate_topic(&long[1..]), Ok(&long[1..]));
}

#[test]
fn nul_is_rejected() {
    assert_eq!(validate_topic("a\0b"), Err(TopicError::Nul));
    assert_eq!(validate_pattern("a/\0"), Err(TopicError::Nul));
}

#[test]
fn wildcard_characters_are_rejected_in_publish_topics() {
    for t in ["+", "#", "a/+", "a/#", "a+b"] {
        assert_eq!(validate_topic(t), Err(TopicError::WildcardInTopic), "{t}");
    }
}

#[test]
fn whole_level_wildcards_are_valid_patterns() {
    for p in [
        "#", "+", "a/+", "a/#", "+/+/+", "+/#", "$mycel/#", "a//b", "psmux/>",
    ] {
        assert_eq!(validate_pattern(p), Ok(p), "{p}");
    }
}

#[test]
fn partial_wildcard_level_is_rejected() {
    for p in ["a+", "a/b#", "+a/b", "a/#b"] {
        assert_eq!(validate_pattern(p), Err(TopicError::PartialWildcard), "{p}");
    }
}

#[test]
fn hash_not_last_is_rejected() {
    // C++ silently treated `a/#/b` as `a/#`.
    assert_eq!(validate_pattern("a/#/b"), Err(TopicError::HashNotLast));
    assert_eq!(validate_pattern("#/a"), Err(TopicError::HashNotLast));
}

#[test]
fn validation_returns_the_topic_without_trailing_slashes() {
    assert_eq!(validate_topic("a/b/"), Ok("a/b"));
    assert_eq!(validate_topic("a/b//"), Ok("a/b"));
    assert_eq!(validate_pattern("a/+/"), Ok("a/+"));
    assert_eq!(validate_pattern("a//b"), Ok("a//b"));
    assert_eq!(validate_topic("/a"), Ok("/a"));
}

#[test]
fn only_slashes_is_an_empty_topic() {
    assert_eq!(validate_topic("/"), Err(TopicError::Empty));
    assert_eq!(validate_pattern("//"), Err(TopicError::Empty));
}

#[test]
fn hash_followed_by_trailing_slash_is_still_last() {
    assert_eq!(validate_pattern("a/#/"), Ok("a/#"));
}

// ── mycel tests/test_topic_trie.cpp, matcher half ──

#[test]
fn exact_match_single_level() {
    assert!(matches("a", "a"));
}

#[test]
fn exact_match_multi_level() {
    assert!(matches("a/b/c", "a/b/c"));
}

#[test]
fn exact_match_no_match() {
    assert!(!matches("a/b/c", "a/b/d"));
    assert!(!matches("a/b", "a/b/c"));
}

#[test]
fn plus_matches_one_level() {
    assert!(matches("a/+/c", "a/b/c"));
}

#[test]
fn plus_does_not_match_multiple_levels() {
    assert!(!matches("a/+", "a/b/c"));
    assert!(!matches("a/+/c", "a/c"));
}

#[test]
fn plus_at_different_positions() {
    assert!(matches("+/b/c", "a/b/c"));
    assert!(matches("a/b/+", "a/b/c"));
    assert!(matches("+/+/+", "a/b/c"));
}

#[test]
fn hash_matches_everything_below() {
    assert!(matches("canopy/task/#", "canopy/task/1"));
    assert!(matches("canopy/task/#", "canopy/task/1/done"));
    assert!(!matches("canopy/task/#", "canopy/other"));
}

#[test]
fn hash_alone_matches_all_non_system_topics() {
    for t in PSMUX_TOPICS {
        assert!(matches("#", t), "{t}");
    }
}

#[test]
fn system_topics_not_matched_by_bare_hash() {
    for t in SYSTEM_TOPICS {
        assert!(!matches("#", t), "{t}");
    }
}

#[test]
fn system_topics_matched_by_explicit_subscription() {
    for t in SYSTEM_TOPICS {
        assert!(matches("$mycel/#", t), "{t}");
    }
}

#[test]
fn system_topics_exact_match() {
    assert!(matches(
        "$mycel/clients/connected",
        "$mycel/clients/connected"
    ));
    assert!(matches("$mycel/+/connected", "$mycel/clients/connected"));
}

// ── spec 3.3 table and 12.6 cases, under the fixed rules ──

#[test]
fn first_level_plus_does_not_match_system_topics() {
    // 12.6 #1 decided the MQTT way: C++ matched both of these.
    assert!(!matches("+/#", "$mycel/clients/connected"));
    assert!(!matches("+/clients/connected", "$mycel/clients/connected"));
    assert!(!matches("+/+/+", "$mycel/clients/connected"));
}

#[test]
fn hash_matches_its_parent_level() {
    assert!(matches("canopy/#", "canopy"));
}

#[test]
fn trailing_slash_is_ignored() {
    assert!(matches("a/b/", "a/b"));
    assert!(matches("a/b", "a/b/"));
}

#[test]
fn deep_topics_match_without_recursion() {
    // ~32k levels, near MAX_TOPIC_LEN; a recursive matcher overflows here.
    let topic = vec!["a"; MAX_TOPIC_LEN / 2].join("/");
    let pattern = vec!["+"; MAX_TOPIC_LEN / 2].join("/");
    assert!(validate_topic(&topic).is_ok());
    assert!(validate_pattern(&pattern).is_ok());
    assert!(matches(&pattern, &topic));
    assert!(matches(&topic, &topic));
    assert!(!matches(&pattern, &format!("{topic}/a")));
}

#[test]
fn leading_slash_is_an_empty_first_level() {
    assert!(!matches("/a", "a"));
    assert!(matches("+/a", "/a"));
}

#[test]
fn empty_middle_level_is_a_real_level() {
    assert!(!matches("a//b", "a/x/b"));
    assert!(matches("a/+/b", "a//b"));
}

#[test]
fn nats_greater_than_is_a_literal_level() {
    assert!(!matches("psmux/>", "psmux/pane/created"));
    assert!(matches("psmux/#", "psmux/pane/created"));
}

#[test]
fn system_topic_detection() {
    assert!(is_system_topic("$mycel/x"));
    assert!(!is_system_topic("mycel/$x"));
}
