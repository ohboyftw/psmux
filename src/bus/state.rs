//! Everything the bus knows, and the handler for every request.
//!
//! No sockets here: connection threads lock one `State`, call
//! [`State::on_message`], and writer threads drain the per-connection
//! queues with [`State::pop`]. Because a request is handled, and all its
//! replies queued, under one lock hold, a reply is always the very next
//! frame after whatever was queued before it (the O1 rule clients rely on).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use super::codec::{encode_frame, fits_in_frame};
use super::proto::{
    Ack, Envelope, Error, Event, Hello, Msg, Publish, Replay, StatusResp, TopicInfo,
};
use super::queue::DEFAULT_EVENT_CAPACITY;
use super::registry::{apply_hello, Registry};
use super::ring::{overflow_reportable, BufferStore, RingConfig};
use super::subs::{ConnId, Subscriptions};
use super::topic;

/// Publisher name on events the bus itself generates.
pub const SERVER_PUBLISHER: &str = "server";
/// Replay high-water marks kept per connection.
const MAX_MARKS_PER_CONN: usize = 8;

/// One encoded frame waiting for a connection's writer.
#[derive(Debug, Clone)]
pub struct Outgoing {
    /// The complete frame, length prefix included.
    pub bytes: Arc<[u8]>,
    /// Stop the bus once this frame is written (the Shutdown Ack).
    pub stop_bus: bool,
}

/// Limits that shape the bus.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Most connections that may complete Hello at once.
    pub max_connections: usize,
    /// Live events queued per connection before the oldest is dropped.
    pub event_capacity: usize,
    /// Per-topic history limits.
    pub ring: RingConfig,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_connections: 100,
            event_capacity: DEFAULT_EVENT_CAPACITY,
            ring: RingConfig::default(),
        }
    }
}

/// What the connection thread must do after a message was handled.
#[derive(Debug)]
pub enum Action {
    /// Nothing beyond waking the writers.
    Continue,
    /// The connection just completed its first Hello: start its writer.
    Registered,
    /// Refused at Hello: write these frame bytes directly, then close.
    Reject(Vec<u8>),
    /// A Shutdown was acknowledged; the bus stops once the Ack is written.
    Shutdown,
}

/// Result of asking for a connection's next frame.
#[derive(Debug)]
pub enum Pop {
    /// Write this frame.
    Frame(Outgoing),
    /// Nothing queued yet.
    Empty,
    /// The connection is no longer registered.
    Gone,
}

/// Replay marks of one connection: pattern and per-topic sequence.
type Marks = Vec<(String, HashMap<String, u64>)>;

/// The bus state behind its one mutex.
#[derive(Debug)]
pub struct State {
    registry: Registry<Outgoing>,
    subs: Subscriptions,
    store: BufferStore,
    marks: HashMap<ConnId, Marks>,
    total_events: u64,
    started: Instant,
    limits: Limits,
    /// Set once the bus is stopping; presence events are no longer sent.
    pub stopping: bool,
}

impl State {
    /// An empty bus.
    pub fn new(limits: Limits) -> Self {
        State {
            registry: Registry::new(),
            subs: Subscriptions::default(),
            store: BufferStore::new(limits.ring),
            marks: HashMap::new(),
            total_events: 0,
            started: Instant::now(),
            limits,
            stopping: false,
        }
    }

    /// Handle one message from `conn`. Before its Hello, everything else
    /// is ignored without a reply, as the C++ server does.
    pub fn on_message(&mut self, conn: ConnId, msg: Msg, now: i64) -> Action {
        if let Msg::Hello(h) = msg {
            return self.hello(conn, &h, now);
        }
        if !self.registry.contains_key(&conn) {
            return Action::Continue;
        }
        match msg {
            Msg::Publish(p) => self.on_publish(conn, p, now),
            Msg::Subscribe(s) => self.on_subscribe(conn, &s.topic_pattern, now),
            Msg::Unsubscribe(u) => self.on_unsubscribe(conn, &u.topic_pattern),
            Msg::Replay(r) => self.on_replay(conn, &r, now),
            Msg::StatusReq(_) => self.on_status(conn, now),
            Msg::Shutdown(_) => {
                self.push_frame(conn, ack("shutdown"), true);
                return Action::Shutdown;
            }
            _ => {}
        }
        Action::Continue
    }

    fn hello(&mut self, conn: ConnId, hello: &Hello, now: i64) -> Action {
        let first = !self.registry.contains_key(&conn);
        if first && self.registry.len() >= self.limits.max_connections {
            let refusal = Msg::Error(Error {
                message: "Server at max connections".into(),
            });
            let bytes = encode_frame(&Envelope { msg: Some(refusal) }).unwrap_or_default();
            return Action::Reject(bytes);
        }
        let cap = self.limits.event_capacity;
        apply_hello(&mut self.registry, conn, hello, now, cap);
        self.push_frame(conn, ack("hello"), false);
        if !first {
            return Action::Continue;
        }
        let payload = serde_json::json!({
            "client_id": hello.client_id, "ts": now, "type": hello.client_type,
        });
        self.publish_system("$mycel/clients/connected", &payload, now);
        Action::Registered
    }

    fn on_publish(&mut self, conn: ConnId, p: Publish, now: i64) {
        // An invalid topic is dropped silently: Publish has no reply, so an
        // Error would be read as the reply to the client's next request.
        let Ok(canonical) = topic::validate_topic(&p.topic) else {
            return;
        };
        let canonical = canonical.to_owned();
        let Some(rec) = self.registry.get_mut(&conn) else {
            return;
        };
        rec.events_published += 1;
        let publisher = rec.client_id.clone();
        self.publish(&canonical, p.payload, p.metadata, &publisher, now);
    }

    fn on_subscribe(&mut self, conn: ConnId, pattern: &str, now: i64) {
        let Ok(pattern) = topic::validate_pattern(pattern) else {
            self.push_frame(conn, error_msg("invalid topic pattern"), false);
            return;
        };
        let pattern = pattern.to_owned();
        let earlier = self.subs.patterns(conn);
        self.subs.subscribe(conn, &pattern);
        self.push_frame(conn, ack("subscribe"), false);
        self.deliver_replay_gap(conn, &pattern, &earlier, now);
    }

    /// After a Replay then Subscribe of the same pattern, deliver what was
    /// published in between, after the Ack. Events an earlier subscription
    /// of this connection already delivered live are skipped.
    fn deliver_replay_gap(&mut self, conn: ConnId, pattern: &str, earlier: &[String], now: i64) {
        let Some(marks) = self.take_marks(conn, pattern) else {
            return;
        };
        let gap = self.store.events_after(pattern, &marks, now);
        for event in gap {
            if earlier.iter().any(|p| topic::matches(p, &event.topic)) {
                continue;
            }
            if let Some(frame) = event_frame(event) {
                self.push_event(conn, &frame);
            }
        }
    }

    fn take_marks(&mut self, conn: ConnId, pattern: &str) -> Option<HashMap<String, u64>> {
        let list = self.marks.get_mut(&conn)?;
        let i = list.iter().position(|(p, _)| p == pattern)?;
        Some(list.remove(i).1)
    }

    fn on_unsubscribe(&mut self, conn: ConnId, pattern: &str) {
        let Ok(pattern) = topic::validate_pattern(pattern) else {
            self.push_frame(conn, error_msg("invalid topic pattern"), false);
            return;
        };
        self.subs.unsubscribe(conn, pattern);
        self.push_frame(conn, ack("unsubscribe"), false);
    }

    fn on_replay(&mut self, conn: ConnId, r: &Replay, now: i64) {
        // An invalid pattern replays nothing but still completes, so the
        // CLI and Python clients waiting for `replay_complete` return.
        if let Ok(pattern) = topic::validate_pattern(&r.topic_pattern) {
            let pattern = pattern.to_owned();
            let events = self
                .store
                .replay(&pattern, r.since_unix_ms, r.max_events, now);
            for frame in events.into_iter().filter_map(event_frame) {
                self.push_frame_bytes(conn, frame, false);
            }
            let marks = self.store.sequence_marks(&pattern);
            self.remember_marks(conn, pattern, marks);
        }
        self.push_frame(conn, ack("replay_complete"), false);
    }

    fn remember_marks(&mut self, conn: ConnId, pattern: String, marks: HashMap<String, u64>) {
        let list = self.marks.entry(conn).or_default();
        list.retain(|(p, _)| *p != pattern);
        if list.len() >= MAX_MARKS_PER_CONN {
            list.remove(0);
        }
        list.push((pattern, marks));
    }

    fn on_status(&mut self, conn: ConnId, now: i64) {
        let status = self.status(now);
        self.push_frame(conn, Msg::StatusResp(status), false);
    }

    /// A status snapshot, as sent in `StatusResp`.
    pub fn status(&mut self, now: i64) -> StatusResp {
        let clients = self
            .registry
            .iter()
            .map(|(conn, rec)| rec.info(self.subs.patterns(*conn)))
            .collect();
        let topics = self
            .store
            .stats(now)
            .into_iter()
            .map(|s| TopicInfo {
                subscriber_count: self.subs.matching(&s.topic).len() as i32,
                topic: s.topic,
                event_count: s.event_count as u64,
                oldest_event_ts: s.oldest_ts,
                newest_event_ts: s.newest_ts,
            })
            .collect();
        StatusResp {
            clients,
            topics,
            uptime_seconds: self.started.elapsed().as_secs(),
            total_events: self.total_events,
        }
    }

    /// Forget a connection. Returns `true` (and announces
    /// `$mycel/clients/disconnected`) only the first time, and only for a
    /// connection that completed Hello.
    pub fn disconnect(&mut self, conn: ConnId, now: i64) -> bool {
        let Some(rec) = self.registry.remove(&conn) else {
            return false;
        };
        self.subs.remove_conn(conn);
        self.marks.remove(&conn);
        if !self.stopping {
            let payload = serde_json::json!({ "client_id": rec.client_id, "ts": now });
            self.publish_system("$mycel/clients/disconnected", &payload, now);
        }
        true
    }

    /// One `$mycel/clients/heartbeat` per registered client.
    pub fn heartbeat(&mut self, now: i64) {
        let ids: Vec<String> = self
            .registry
            .values()
            .map(|r| r.client_id.clone())
            .collect();
        for id in ids {
            let payload = serde_json::json!({ "client_id": id, "ts": now });
            self.publish_system("$mycel/clients/heartbeat", &payload, now);
        }
    }

    /// Announce `$mycel/server/started`.
    pub fn server_started(&mut self, port: u16, bind: &str, now: i64) {
        let payload = serde_json::json!({
            "client_id": SERVER_PUBLISHER, "ts": now, "port": port, "bind": bind,
        });
        self.publish_system("$mycel/server/started", &payload, now);
    }

    /// The next frame for `conn`'s writer.
    pub fn pop(&mut self, conn: ConnId) -> Pop {
        match self.registry.get_mut(&conn) {
            None => Pop::Gone,
            Some(rec) => rec.queue.pop().map_or(Pop::Empty, Pop::Frame),
        }
    }

    /// Control and replay frames waiting for `conn` (0 if unknown).
    pub fn queued_control(&self, conn: ConnId) -> usize {
        self.registry
            .get(&conn)
            .map_or(0, |r| r.queue.queued_control())
    }

    /// Frames of either kind waiting for `conn` (0 if unknown).
    pub fn queued(&self, conn: ConnId) -> usize {
        self.registry.get(&conn).map_or(0, |r| r.queue.len())
    }

    fn publish_system(&mut self, topic: &str, payload: &serde_json::Value, now: i64) {
        let bytes = serde_json::to_vec(payload).unwrap_or_default();
        self.publish(topic, bytes, HashMap::new(), SERVER_PUBLISHER, now);
    }

    /// Sequence, store and fan out one event on a canonical topic.
    fn publish(
        &mut self,
        topic: &str,
        payload: Vec<u8>,
        metadata: HashMap<String, String>,
        publisher: &str,
        now: i64,
    ) {
        let event = Event {
            topic: topic.into(),
            payload,
            ts_unix_ms: now,
            metadata,
            sequence: 0,
            publisher: publisher.into(),
        };
        let Some(mut event) = fit_in_frame(event) else {
            return;
        };
        event.sequence = self.store.next_sequence(topic);
        let stored = self.store.store(event.clone(), now);
        event.ts_unix_ms = stored.ts_unix_ms;
        self.total_events += 1;
        self.fan_out(event);
        if stored.evicted.by_count > 0 && overflow_reportable(topic) {
            self.announce_overflow(topic, stored.evicted.by_count, now);
        }
    }

    /// `$mycel/server/buffer_overflow`. Its own evictions are never
    /// reported (a `$`-topic), so this recurses at most once.
    fn announce_overflow(&mut self, topic: &str, evicted: usize, now: i64) {
        let payload = serde_json::json!({
            "client_id": SERVER_PUBLISHER, "ts": now, "topic": topic, "evicted": evicted,
        });
        self.publish_system("$mycel/server/buffer_overflow", &payload, now);
    }

    fn fan_out(&mut self, event: Event) {
        let conns = self.subs.matching(&event.topic);
        if conns.is_empty() {
            return;
        }
        let Some(frame) = event_frame(event) else {
            return;
        };
        for conn in conns {
            self.push_event(conn, &frame);
        }
    }

    fn push_event(&mut self, conn: ConnId, frame: &Arc<[u8]>) {
        if let Some(rec) = self.registry.get_mut(&conn) {
            rec.queue.push_event(Outgoing {
                bytes: frame.clone(),
                stop_bus: false,
            });
            rec.events_received += 1;
        }
    }

    fn push_frame(&mut self, conn: ConnId, msg: Msg, stop_bus: bool) {
        if let Ok(bytes) = encode_frame(&Envelope { msg: Some(msg) }) {
            self.push_frame_bytes(conn, bytes.into(), stop_bus);
        }
    }

    fn push_frame_bytes(&mut self, conn: ConnId, bytes: Arc<[u8]>, stop_bus: bool) {
        if let Some(rec) = self.registry.get_mut(&conn) {
            rec.queue.push_control(Outgoing { bytes, stop_bus });
        }
    }
}

fn ack(reference: &str) -> Msg {
    Msg::Ack(Ack {
        r#ref: reference.into(),
    })
}

fn error_msg(message: &str) -> Msg {
    Msg::Error(Error {
        message: message.into(),
    })
}

/// `event` back if its envelope still fits in a frame once the bus sets
/// the largest possible sequence number; `None` if it never could.
fn fit_in_frame(mut event: Event) -> Option<Event> {
    event.sequence = u64::MAX;
    let env = Envelope {
        msg: Some(Msg::Event(event)),
    };
    if !fits_in_frame(&env) {
        return None;
    }
    match env.msg {
        Some(Msg::Event(e)) => Some(e),
        _ => None,
    }
}

fn event_frame(event: Event) -> Option<Arc<[u8]>> {
    encode_frame(&Envelope {
        msg: Some(Msg::Event(event)),
    })
    .ok()
    .map(Arc::from)
}

#[cfg(test)]
#[path = "../../tests-rs/test_bus_state.rs"]
mod tests;
