// Retention tests for src/bus/ring.rs.
// Ported from mycel tests/test_ring_buffer.cpp, plus spec 12.6 #2, #4, #5,
// #12 and the wall-clock TTL fix.

use super::*;

const NO_TTL: RingConfig = RingConfig {
    max_events: 100,
    ttl_ms: 0,
};

fn ev(topic: &str, ts: i64, seq: u64) -> Event {
    Event {
        topic: topic.into(),
        ts_unix_ms: ts,
        sequence: seq,
        ..Default::default()
    }
}

fn ring_with(cfg: RingConfig, stamps: &[i64]) -> RingBuffer {
    let mut r = RingBuffer::new(cfg);
    for (i, ts) in stamps.iter().enumerate() {
        r.push(ev("t", *ts, i as u64 + 1), *ts);
    }
    r
}

fn stamps(events: &[Event]) -> Vec<i64> {
    events.iter().map(|e| e.ts_unix_ms).collect()
}

fn seqs(events: &[Event]) -> Vec<(String, u64)> {
    events
        .iter()
        .map(|e| (e.topic.clone(), e.sequence))
        .collect()
}

// ── RingBuffer ──

#[test]
fn push_and_size() {
    let r = ring_with(NO_TTL, &[1000, 2000]);
    assert_eq!(r.len(), 2);
}

#[test]
fn total_count_includes_evicted() {
    let cfg = RingConfig {
        max_events: 3,
        ttl_ms: 0,
    };
    let r = ring_with(cfg, &[1000, 2000, 3000, 4000, 5000]);
    assert_eq!(r.len(), 3);
    assert_eq!(r.total_pushed(), 5);
}

#[test]
fn count_eviction_keeps_newest() {
    let cfg = RingConfig {
        max_events: 3,
        ttl_ms: 0,
    };
    let r = ring_with(cfg, &[1000, 2000, 3000, 4000, 5000]);
    assert_eq!(r.oldest_ts(), 3000);
    assert_eq!(r.newest_ts(), 5000);
}

#[test]
fn push_reports_evicted_count() {
    let cfg = RingConfig {
        max_events: 1,
        ttl_ms: 0,
    };
    let mut r = RingBuffer::new(cfg);
    assert_eq!(r.push(ev("t", 1, 1), 1).evicted.by_count, 0);
    assert_eq!(r.push(ev("t", 2, 2), 2).evicted.by_count, 1);
}

#[test]
fn ttl_expiry_is_not_counted_as_overflow() {
    let cfg = RingConfig {
        max_events: 100,
        ttl_ms: 5000,
    };
    let mut r = ring_with(cfg, &[1000, 2000]);
    let evicted = r.push(ev("t", 9000, 3), 9000).evicted;
    assert_eq!(
        evicted,
        Evicted {
            by_count: 0,
            by_ttl: 2
        }
    );
}

#[test]
fn push_returns_the_clamped_timestamp_it_stored() {
    let mut r = RingBuffer::new(NO_TTL);
    r.push(ev("t", 5000, 1), 5000);
    assert_eq!(r.push(ev("t", 4000, 2), 5000).ts_unix_ms, 5000);
}

#[test]
fn ttl_eviction_on_push() {
    let cfg = RingConfig {
        max_events: 100,
        ttl_ms: 5000,
    };
    let r = ring_with(cfg, &[1000, 4000, 7000]);
    assert_eq!(r.len(), 2);
    assert_eq!(r.oldest_ts(), 4000);
}

#[test]
fn ttl_is_measured_from_now_not_newest_event() {
    // C++ kept an idle topic's events forever.
    let cfg = RingConfig {
        max_events: 100,
        ttl_ms: 5000,
    };
    let mut r = ring_with(cfg, &[1000, 2000]);
    assert!(r.replay(0, 0, 100_000).is_empty());
    assert!(r.is_empty());
}

#[test]
fn ttl_zero_disables_ttl_eviction() {
    let mut r = ring_with(NO_TTL, &[1]);
    assert_eq!(r.replay(0, 0, i64::MAX).len(), 1);
}

#[test]
fn oldest_newest_empty() {
    let r = RingBuffer::new(NO_TTL);
    assert_eq!((r.oldest_ts(), r.newest_ts()), (0, 0));
}

#[test]
fn replay_since_filter() {
    let mut r = ring_with(NO_TTL, &[1000, 2000, 3000, 4000]);
    assert_eq!(stamps(&r.replay(2500, 0, 4000)), [3000, 4000]);
}

#[test]
fn replay_since_is_inclusive() {
    let mut r = ring_with(NO_TTL, &[1000, 2000, 3000]);
    assert_eq!(stamps(&r.replay(2000, 0, 3000)), [2000, 3000]);
}

#[test]
fn replay_max_events_takes_oldest_first() {
    let mut r = ring_with(NO_TTL, &[1000, 2000, 3000, 4000]);
    assert_eq!(stamps(&r.replay(0, 2, 4000)), [1000, 2000]);
}

#[test]
fn replay_all_when_max_zero() {
    let mut r = ring_with(NO_TTL, &[1000, 2000, 3000]);
    assert_eq!(r.replay(0, 0, 3000).len(), 3);
}

#[test]
fn replay_empty() {
    let mut r = RingBuffer::new(NO_TTL);
    assert!(r.replay(0, 0, 0).is_empty());
}

#[test]
fn timestamps_never_go_backwards_within_a_topic() {
    let mut r = RingBuffer::new(NO_TTL);
    r.push(ev("t", 5000, 1), 5000);
    r.push(ev("t", 4000, 2), 5000);
    assert_eq!(stamps(&r.replay(0, 0, 5000)), [5000, 5000]);
}

// ── BufferStore ──

fn store_with(events: &[(&str, i64)]) -> BufferStore {
    let mut s = BufferStore::new(NO_TTL);
    for (i, (topic, ts)) in events.iter().enumerate() {
        s.store(ev(topic, *ts, i as u64 + 1), *ts);
    }
    s
}

#[test]
fn store_and_replay_single_topic() {
    let mut s = store_with(&[("a/b", 1000), ("a/b", 2000)]);
    assert_eq!(stamps(&s.replay("a/b", 0, 0, 2000)), [1000, 2000]);
}

#[test]
fn replay_nonexistent_topic() {
    let mut s = store_with(&[("a/b", 1000)]);
    assert!(s.replay("x/y", 0, 0, 1000).is_empty());
}

#[test]
fn replay_pattern_hash() {
    let mut s = store_with(&[("a/b", 1000), ("a/c/d", 2000), ("x", 3000)]);
    assert_eq!(s.replay("a/#", 0, 0, 3000).len(), 2);
}

#[test]
fn replay_pattern_plus() {
    let mut s = store_with(&[("a/b", 1000), ("a/c/d", 2000), ("a/e", 3000)]);
    assert_eq!(stamps(&s.replay("a/+", 0, 0, 3000)), [1000, 3000]);
}

#[test]
fn replay_pattern_sorted_by_time_across_topics() {
    let mut s = store_with(&[("a/x", 3000), ("a/y", 1000), ("a/x", 4000), ("a/y", 2000)]);
    assert_eq!(
        stamps(&s.replay("a/#", 0, 0, 4000)),
        [1000, 2000, 3000, 4000]
    );
}

#[test]
fn same_millisecond_replay_keeps_sequence_order() {
    let mut s = BufferStore::new(NO_TTL);
    for seq in 1..=50 {
        s.store(ev("a/x", 1000, seq), 1000);
        s.store(ev("a/y", 1000, seq), 1000);
    }
    let out = s.replay("a/#", 0, 0, 1000);
    let x: Vec<u64> = out
        .iter()
        .filter(|e| e.topic == "a/x")
        .map(|e| e.sequence)
        .collect();
    assert_eq!(x, (1..=50).collect::<Vec<_>>());
}

#[test]
fn max_events_across_topics_is_oldest_n_and_deterministic() {
    let mut s = store_with(&[("b", 1000), ("a", 2000), ("c", 3000), ("a", 4000)]);
    let first = seqs(&s.replay("#", 0, 2, 4000));
    assert_eq!(first, [("b".to_string(), 1), ("a".to_string(), 2)]);
    assert_eq!(seqs(&s.replay("#", 0, 2, 4000)), first);
}

#[test]
fn replay_hash_excludes_system_topics() {
    // 12.6 #2 decided the MQTT way: C++ replay '#' returned $mycel/* events.
    let mut s = store_with(&[("$mycel/clients/heartbeat", 1000), ("a", 2000)]);
    assert_eq!(seqs(&s.replay("#", 0, 0, 2000)), [("a".to_string(), 2)]);
    assert_eq!(s.replay("$mycel/#", 0, 0, 2000).len(), 1);
}

#[test]
fn stats_report_each_buffered_topic() {
    let mut s = store_with(&[("b", 1000), ("a", 2000), ("a", 3000)]);
    let stats = s.stats(3000);
    let a = TopicStats {
        topic: "a".into(),
        event_count: 2,
        oldest_ts: 2000,
        newest_ts: 3000,
    };
    assert_eq!(stats[0], a);
    assert_eq!(stats.len(), 2);
}

#[test]
fn stats_forget_topics_that_expired() {
    let cfg = RingConfig {
        max_events: 10,
        ttl_ms: 1000,
    };
    let mut s = BufferStore::new(cfg);
    s.store(ev("old", 1000, 1), 1000);
    s.store(ev("new", 5000, 1), 5000);
    let topics: Vec<String> = s.stats(5000).into_iter().map(|t| t.topic).collect();
    assert_eq!(topics, ["new"]);
}

#[test]
fn store_reports_evictions_for_its_topic() {
    let cfg = RingConfig {
        max_events: 2,
        ttl_ms: 0,
    };
    let mut s = BufferStore::new(cfg);
    let evicted: Vec<usize> = (1..=4)
        .map(|i| s.store(ev("a", i, i as u64), i).evicted.by_count)
        .collect();
    assert_eq!(evicted, [0, 0, 1, 1]);
}

#[test]
fn sequences_count_per_topic_from_one() {
    let mut s = BufferStore::new(NO_TTL);
    let got: Vec<u64> = ["a", "b", "a"].iter().map(|t| s.next_sequence(t)).collect();
    assert_eq!(got, [1, 1, 2]);
}

#[test]
fn sequence_keeps_counting_after_the_topic_expires() {
    let cfg = RingConfig {
        max_events: 10,
        ttl_ms: 1000,
    };
    let mut s = BufferStore::new(cfg);
    let first = s.next_sequence("a");
    s.store(ev("a", 1000, first), 1000);
    assert!(s.stats(5000).is_empty());
    assert_eq!(s.next_sequence("a"), first + 1);
}

#[test]
fn store_replay_drops_events_past_the_ttl() {
    let cfg = RingConfig {
        max_events: 10,
        ttl_ms: 1000,
    };
    let mut s = BufferStore::new(cfg);
    s.store(ev("a", 1000, 1), 1000);
    s.store(ev("a", 4500, 2), 4500);
    assert_eq!(seqs(&s.replay("a", 0, 0, 5000)), [("a".to_string(), 2)]);
}

// ── overflow storm guard (12.6 #12) ──
// Only the predicate is tested here. That sustained eviction does not
// recurse through $mycel/server/buffer_overflow is owed by M2's
// publish-path tests.

#[test]
fn evictions_from_system_topics_are_never_reported() {
    let cfg = RingConfig {
        max_events: 2,
        ttl_ms: 0,
    };
    let mut s = BufferStore::new(cfg);
    let topic = "$mycel/server/buffer_overflow";
    let evicted: usize = (1..=10)
        .map(|i| s.store(ev(topic, i, i as u64), i).evicted.by_count)
        .sum();
    assert_eq!(evicted, 8);
    assert!(!overflow_reportable(topic));
    assert!(overflow_reportable("psmux/pane/created"));
}
