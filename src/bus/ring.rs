//! Per-topic event history with a count limit and a wall-clock TTL.
//!
//! Differences from the C++ `BufferStore`:
//! - TTL is measured from the caller's `now`, not from the topic's newest
//!   event, and is applied on push, on replay and on stats. An idle topic
//!   therefore ages out instead of keeping its last events forever.
//! - Timestamps inside one topic never go backwards (a push is clamped to
//!   the newest buffered timestamp), so sorting replay output by time with
//!   a stable sort keeps each topic in sequence order.
//! - `max_events` keeps the oldest N events of the sorted result, which is
//!   deterministic. MCP callers asking for "the last 100" get the oldest
//!   100 in the window; this matches C++ and is left for M2/M3 to revisit.
//!   Topics whose buffers expire to empty are dropped.
//! - Per-topic sequence numbers live in [`BufferStore::next_sequence`]'s
//!   own map, which expiry never touches, so a topic that ages out and is
//!   published again keeps counting up instead of restarting at 1.
//!   Dedup by `(topic, sequence)` across replay and live delivery relies
//!   on this. The map holds one entry per topic ever published.
//! - Keys are canonical topics (see [`topic::validate_topic`]).

use std::collections::{HashMap, VecDeque};

use super::proto::Event;
use super::topic;

/// Default events kept per topic.
pub const DEFAULT_MAX_EVENTS: usize = 10_000;
/// Default event lifetime, in ms (10 minutes).
pub const DEFAULT_TTL_MS: i64 = 600_000;

/// Retention limits shared by every topic buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RingConfig {
    /// Events kept per topic. Older events are evicted first.
    pub max_events: usize,
    /// Event lifetime in ms; `<= 0` disables TTL eviction.
    pub ttl_ms: i64,
}

impl Default for RingConfig {
    fn default() -> Self {
        RingConfig {
            max_events: DEFAULT_MAX_EVENTS,
            ttl_ms: DEFAULT_TTL_MS,
        }
    }
}

/// Events evicted by one push, split by cause.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Evicted {
    /// Dropped because the buffer was full. Only these are an overflow
    /// worth announcing on `$mycel/server/buffer_overflow`.
    pub by_count: usize,
    /// Dropped because they outlived the TTL: ordinary ageing, not an
    /// overflow.
    pub by_ttl: usize,
}

/// What a push actually stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stored {
    /// The timestamp stored, after clamping to the newest buffered one.
    /// The live fan-out copy must carry this value, so live and replayed
    /// copies of one `(topic, sequence)` agree.
    pub ts_unix_ms: i64,
    /// Events evicted to make room or by TTL.
    pub evicted: Evicted,
}

/// History of one concrete topic, oldest first.
#[derive(Debug)]
pub struct RingBuffer {
    cfg: RingConfig,
    events: VecDeque<Event>,
    total_pushed: u64,
}

impl RingBuffer {
    /// An empty buffer with the given limits.
    pub fn new(cfg: RingConfig) -> Self {
        RingBuffer {
            cfg,
            events: VecDeque::new(),
            total_pushed: 0,
        }
    }

    /// Append an event and evict by count and TTL.
    pub fn push(&mut self, mut event: Event, now_ms: i64) -> Stored {
        if let Some(newest) = self.events.back() {
            event.ts_unix_ms = event.ts_unix_ms.max(newest.ts_unix_ms);
        }
        let ts_unix_ms = event.ts_unix_ms;
        self.events.push_back(event);
        self.total_pushed += 1;
        let mut by_count = 0;
        while self.events.len() > self.cfg.max_events {
            self.events.pop_front();
            by_count += 1;
        }
        let by_ttl = self.expire(now_ms);
        Stored {
            ts_unix_ms,
            evicted: Evicted { by_count, by_ttl },
        }
    }

    /// Drop events older than `now_ms - ttl`. Returns how many were dropped.
    pub fn expire(&mut self, now_ms: i64) -> usize {
        if self.cfg.ttl_ms <= 0 {
            return 0;
        }
        let cutoff = now_ms.saturating_sub(self.cfg.ttl_ms);
        let before = self.events.len();
        while self.events.front().is_some_and(|e| e.ts_unix_ms < cutoff) {
            self.events.pop_front();
        }
        before - self.events.len()
    }

    /// Events currently buffered.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// True when nothing is buffered.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Events pushed into this buffer, evicted ones included. Resets when
    /// the buffer expires and is dropped; use
    /// [`BufferStore::next_sequence`] for sequence numbers.
    pub fn total_pushed(&self) -> u64 {
        self.total_pushed
    }

    /// Timestamp of the oldest buffered event, or 0.
    pub fn oldest_ts(&self) -> i64 {
        self.events.front().map_or(0, |e| e.ts_unix_ms)
    }

    /// Timestamp of the newest buffered event, or 0.
    pub fn newest_ts(&self) -> i64 {
        self.events.back().map_or(0, |e| e.ts_unix_ms)
    }

    /// Unexpired events with `ts >= since_ms` (inclusive), oldest first,
    /// at most `max` of them when `max > 0`.
    pub fn replay(&mut self, since_ms: i64, max: usize, now_ms: i64) -> Vec<Event> {
        self.expire(now_ms);
        let limit = if max == 0 { usize::MAX } else { max };
        self.since(since_ms).take(limit).cloned().collect()
    }

    fn since(&self, since_ms: i64) -> impl Iterator<Item = &Event> {
        self.events.iter().filter(move |e| e.ts_unix_ms >= since_ms)
    }
}

/// Buffer statistics for one topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicStats {
    /// Concrete topic.
    pub topic: String,
    /// Events currently buffered.
    pub event_count: usize,
    /// Oldest buffered timestamp, or 0.
    pub oldest_ts: i64,
    /// Newest buffered timestamp, or 0.
    pub newest_ts: i64,
}

/// One [`RingBuffer`] per concrete topic, created on first store, plus
/// the per-topic sequence counters.
#[derive(Debug, Default)]
pub struct BufferStore {
    cfg: RingConfig,
    buffers: HashMap<String, RingBuffer>,
    sequences: HashMap<String, u64>,
}

impl BufferStore {
    /// An empty store whose buffers use `cfg`.
    pub fn new(cfg: RingConfig) -> Self {
        BufferStore {
            cfg,
            buffers: HashMap::new(),
            sequences: HashMap::new(),
        }
    }

    /// The next sequence number for `topic`, starting at 1. Never reset,
    /// not even when the topic's buffer expires.
    pub fn next_sequence(&mut self, topic: &str) -> u64 {
        if let Some(seq) = self.sequences.get_mut(topic) {
            *seq += 1;
            return *seq;
        }
        self.sequences.insert(topic.to_owned(), 1);
        1
    }

    /// Buffer an event under its (canonical) topic.
    pub fn store(&mut self, event: Event, now_ms: i64) -> Stored {
        let cfg = self.cfg;
        self.buffers
            .entry(event.topic.clone())
            .or_insert_with(|| RingBuffer::new(cfg))
            .push(event, now_ms)
    }

    /// Unexpired events of every topic matching `pattern` with
    /// `ts >= since_ms`, sorted by time (stable, so each topic stays in
    /// sequence order) and cut to the oldest `max_events` when it is > 0.
    pub fn replay(
        &mut self,
        pattern: &str,
        since_ms: i64,
        max_events: i32,
        now_ms: i64,
    ) -> Vec<Event> {
        self.prune(now_ms);
        let mut topics: Vec<&String> = self
            .buffers
            .keys()
            .filter(|t| topic::matches(pattern, t))
            .collect();
        topics.sort();
        let mut out: Vec<Event> = topics
            .into_iter()
            .flat_map(|t| self.buffers[t].since(since_ms).cloned())
            .collect();
        out.sort_by_key(|e| e.ts_unix_ms);
        if max_events > 0 {
            out.truncate(max_events as usize);
        }
        out
    }

    /// Statistics for every topic with buffered events, sorted by topic.
    pub fn stats(&mut self, now_ms: i64) -> Vec<TopicStats> {
        self.prune(now_ms);
        let mut stats: Vec<TopicStats> = self
            .buffers
            .iter()
            .map(|(topic, buf)| TopicStats {
                topic: topic.clone(),
                event_count: buf.len(),
                oldest_ts: buf.oldest_ts(),
                newest_ts: buf.newest_ts(),
            })
            .collect();
        stats.sort_by(|a, b| a.topic.cmp(&b.topic));
        stats
    }

    /// Apply TTL to every buffer and forget topics left empty.
    fn prune(&mut self, now_ms: i64) {
        self.buffers.retain(|_, buf| {
            buf.expire(now_ms);
            !buf.is_empty()
        });
    }
}

/// Whether an eviction from `topic` may be announced on
/// `$mycel/server/buffer_overflow`. Never for `$`-topics: announcing those
/// is what made the C++ server recurse without bound once the overflow
/// topic's own buffer filled.
pub fn overflow_reportable(topic: &str) -> bool {
    !topic::is_system_topic(topic)
}

#[cfg(test)]
#[path = "../../tests-rs/test_bus_ring.rs"]
mod tests;
