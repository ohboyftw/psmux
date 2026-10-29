//! Wire messages of the mycel protocol.
//!
//! Hand-written prost structs that mirror `proto/mycel.proto` (package
//! `mycel`, proto3) field number for field number. The field numbers are
//! the wire contract; the Rust names only need to read well.
//!
//! One addition: `ClientInfo.events_dropped = 7`. proto3 readers skip
//! unknown fields, so existing clients are unaffected.

use std::collections::HashMap;

/// Every frame on the wire carries exactly one `Envelope`.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Envelope {
    /// The message inside the envelope. `None` for an empty frame.
    #[prost(oneof = "Msg", tags = "1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11")]
    pub msg: Option<Msg>,
}

/// The `oneof msg` of `Envelope`.
#[derive(Clone, PartialEq, prost::Oneof)]
pub enum Msg {
    /// Client to server: first frame on a connection.
    #[prost(message, tag = "1")]
    Hello(Hello),
    /// Client to server: start live delivery for a pattern.
    #[prost(message, tag = "2")]
    Subscribe(Subscribe),
    /// Client to server: stop live delivery for a pattern.
    #[prost(message, tag = "3")]
    Unsubscribe(Unsubscribe),
    /// Client to server: publish one event (no reply).
    #[prost(message, tag = "4")]
    Publish(Publish),
    /// Client to server: ask for buffered history.
    #[prost(message, tag = "5")]
    Replay(Replay),
    /// Server to client: one delivered event.
    #[prost(message, tag = "6")]
    Event(Event),
    /// Client to server: ask for server status.
    #[prost(message, tag = "7")]
    StatusReq(StatusReq),
    /// Server to client: answer to `StatusReq`.
    #[prost(message, tag = "8")]
    StatusResp(StatusResp),
    /// Server to client: acknowledgement of a request.
    #[prost(message, tag = "9")]
    Ack(Ack),
    /// Server to client: a refusal.
    #[prost(message, tag = "10")]
    Error(Error),
    /// Client to server: stop the bus.
    #[prost(message, tag = "11")]
    Shutdown(Shutdown),
}

/// Handshake. `client_type` is free text; every known client sends `"both"`.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Hello {
    /// Name the client reports as `publisher` on its events.
    #[prost(string, tag = "1")]
    pub client_id: String,
    /// Free-text role of the client.
    #[prost(string, tag = "2")]
    pub client_type: String,
}

/// Subscribe to a topic pattern.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Subscribe {
    /// Pattern with optional `+` and `#` wildcard levels.
    #[prost(string, tag = "1")]
    pub topic_pattern: String,
}

/// Remove one subscription pattern.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Unsubscribe {
    /// The exact pattern string that was subscribed.
    #[prost(string, tag = "1")]
    pub topic_pattern: String,
}

/// Publish one event.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Publish {
    /// Concrete topic, no wildcards.
    #[prost(string, tag = "1")]
    pub topic: String,
    /// Opaque payload bytes.
    #[prost(bytes = "vec", tag = "2")]
    pub payload: Vec<u8>,
    /// Client clock. The server ignores it and stamps its own time.
    #[prost(int64, tag = "3")]
    pub ts_unix_ms: i64,
    /// Free-form key/value pairs.
    #[prost(map = "string, string", tag = "4")]
    pub metadata: HashMap<String, String>,
}

/// Ask for buffered events.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Replay {
    /// Pattern with optional `+` and `#` wildcard levels.
    #[prost(string, tag = "1")]
    pub topic_pattern: String,
    /// Inclusive lower bound on the event timestamp, Unix ms.
    #[prost(int64, tag = "2")]
    pub since_unix_ms: i64,
    /// Upper bound on returned events; `<= 0` means no limit.
    #[prost(int32, tag = "3")]
    pub max_events: i32,
}

/// Ask for server status. Has no fields.
#[derive(Clone, PartialEq, prost::Message)]
pub struct StatusReq {}

/// Stop the bus.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Shutdown {
    /// Free-text reason, logged by the server.
    #[prost(string, tag = "1")]
    pub reason: String,
}

/// One delivered event.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Event {
    /// Concrete topic.
    #[prost(string, tag = "1")]
    pub topic: String,
    /// Opaque payload bytes.
    #[prost(bytes = "vec", tag = "2")]
    pub payload: Vec<u8>,
    /// Server receive time, Unix ms.
    #[prost(int64, tag = "3")]
    pub ts_unix_ms: i64,
    /// Key/value pairs copied from the `Publish`.
    #[prost(map = "string, string", tag = "4")]
    pub metadata: HashMap<String, String>,
    /// Per-topic sequence number, starting at 1.
    #[prost(uint64, tag = "5")]
    pub sequence: u64,
    /// `Hello.client_id` of the publisher, or `"server"`.
    #[prost(string, tag = "6")]
    pub publisher: String,
}

/// Acknowledgement. `ref` names the request (`"hello"`, `"subscribe"`, ...).
#[derive(Clone, PartialEq, prost::Message)]
pub struct Ack {
    /// Which request this acknowledges.
    #[prost(string, tag = "1")]
    pub r#ref: String,
}

/// A refusal sent by the server.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Error {
    /// Human-readable reason.
    #[prost(string, tag = "1")]
    pub message: String,
}

/// Server status snapshot.
#[derive(Clone, PartialEq, prost::Message)]
pub struct StatusResp {
    /// One entry per connected client.
    #[prost(message, repeated, tag = "1")]
    pub clients: Vec<ClientInfo>,
    /// One entry per buffered topic.
    #[prost(message, repeated, tag = "2")]
    pub topics: Vec<TopicInfo>,
    /// Seconds since the bus started.
    #[prost(uint64, tag = "3")]
    pub uptime_seconds: u64,
    /// Events published since the bus started, system events included.
    #[prost(uint64, tag = "4")]
    pub total_events: u64,
}

/// One connected client in a `StatusResp`.
#[derive(Clone, PartialEq, prost::Message)]
pub struct ClientInfo {
    /// `Hello.client_id`.
    #[prost(string, tag = "1")]
    pub client_id: String,
    /// `Hello.client_type`.
    #[prost(string, tag = "2")]
    pub client_type: String,
    /// Connect time, Unix ms.
    #[prost(int64, tag = "3")]
    pub connected_at: i64,
    /// Live subscription patterns.
    #[prost(string, repeated, tag = "4")]
    pub subscriptions: Vec<String>,
    /// Publishes received from this client.
    #[prost(uint64, tag = "5")]
    pub events_published: u64,
    /// Live events queued to this client.
    #[prost(uint64, tag = "6")]
    pub events_received: u64,
    /// Live events dropped from this client's full queue (psmux addition).
    #[prost(uint64, tag = "7")]
    pub events_dropped: u64,
}

/// One buffered topic in a `StatusResp`.
#[derive(Clone, PartialEq, prost::Message)]
pub struct TopicInfo {
    /// Concrete topic.
    #[prost(string, tag = "1")]
    pub topic: String,
    /// Events currently buffered for the topic.
    #[prost(uint64, tag = "2")]
    pub event_count: u64,
    /// Live subscribers whose pattern matches the topic.
    #[prost(int32, tag = "3")]
    pub subscriber_count: i32,
    /// Timestamp of the oldest buffered event, or 0.
    #[prost(int64, tag = "4")]
    pub oldest_event_ts: i64,
    /// Timestamp of the newest buffered event, or 0.
    #[prost(int64, tag = "5")]
    pub newest_event_ts: i64,
}
