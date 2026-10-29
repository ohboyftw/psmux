// Subscription table tests for src/bus/subs.rs.
// Ported from mycel tests/test_topic_trie.cpp (subscriber half), with the
// fixed per-pattern unsubscribe and once-per-connection delivery.

use super::*;

fn table(subs: &[(ConnId, &str)]) -> Subscriptions {
    let mut t = Subscriptions::default();
    for (c, p) in subs {
        t.subscribe(*c, p);
    }
    t
}

#[test]
fn exact_subscription_is_notified() {
    let t = table(&[(1, "a/b")]);
    assert_eq!(t.matching("a/b"), [1]);
    assert!(t.matching("a/c").is_empty());
}

#[test]
fn unsubscribe_removes_delivery() {
    let mut t = table(&[(1, "a/b")]);
    assert!(t.unsubscribe(1, "a/b"));
    assert!(t.matching("a/b").is_empty());
}

#[test]
fn unsubscribe_hash_wildcard() {
    let mut t = table(&[(1, "a/#")]);
    assert!(t.unsubscribe(1, "a/#"));
    assert!(t.matching("a/b/c").is_empty());
}

#[test]
fn unsubscribe_removes_only_the_named_pattern() {
    // C++ removed every trie entry of the connection here.
    let mut t = table(&[(1, "a"), (1, "b")]);
    t.unsubscribe(1, "a");
    assert!(t.matching("a").is_empty());
    assert_eq!(t.matching("b"), [1]);
    assert_eq!(t.patterns(1), ["b"]);
}

#[test]
fn unsubscribe_unknown_pattern_reports_false() {
    let mut t = table(&[(1, "a")]);
    assert!(!t.unsubscribe(1, "zzz"));
    assert!(!t.unsubscribe(2, "a"));
    assert_eq!(t.matching("a"), [1]);
}

#[test]
fn multiple_subscribers_all_notified() {
    let t = table(&[(1, "a/b"), (2, "a/b"), (3, "a/b")]);
    assert_eq!(t.matching("a/b"), [1, 2, 3]);
}

#[test]
fn mixed_exact_and_wildcard_subscribers() {
    let t = table(&[(1, "a/b"), (2, "a/+"), (3, "a/#")]);
    assert_eq!(t.matching("a/b"), [1, 2, 3]);
}

#[test]
fn match_count_is_accurate() {
    let t = table(&[(1, "a/b/c"), (2, "a/+/c"), (3, "a/#"), (4, "#"), (5, "x/#")]);
    assert_eq!(t.matching("a/b/c").len(), 4);
}

#[test]
fn duplicate_subscribe_is_one_subscription() {
    let mut t = table(&[(1, "a")]);
    assert!(!t.subscribe(1, "a"));
    assert_eq!(t.patterns(1), ["a"]);
    assert_eq!(t.matching("a"), [1]);
}

#[test]
fn overlapping_patterns_deliver_once_per_connection() {
    let t = table(&[(1, "a/#"), (1, "a/b"), (1, "+/b")]);
    assert_eq!(t.matching("a/b"), [1]);
}

#[test]
fn remove_conn_drops_all_its_patterns() {
    let mut t = table(&[(1, "a"), (1, "b"), (2, "a")]);
    t.remove_conn(1);
    assert!(t.patterns(1).is_empty());
    assert_eq!(t.matching("a"), [2]);
}

#[test]
fn bare_hash_subscriber_does_not_get_system_topics() {
    let t = table(&[(1, "#"), (2, "$mycel/#")]);
    assert_eq!(t.matching("$mycel/clients/connected"), [2]);
    assert_eq!(t.matching("psmux/pane/created"), [1]);
}
