//! Per-connection client state, keyed by connection id.
//!
//! Each connection has exactly one record, so a second Hello on the same
//! connection replaces the record instead of orphaning the first one as the
//! C++ server did. Records are kept in a `BTreeMap` so status output is in
//! connection order.

use std::collections::BTreeMap;

use super::proto::{ClientInfo, Hello};
use super::queue::OutboundQueue;
use super::subs::ConnId;

/// State of one connection that completed Hello.
#[derive(Debug)]
pub struct ClientRecord<T> {
    /// `Hello.client_id`, used as `publisher` on its events.
    pub client_id: String,
    /// `Hello.client_type`.
    pub client_type: String,
    /// Hello time, Unix ms.
    pub connected_at: i64,
    /// Publishes received from this client.
    pub events_published: u64,
    /// Live events queued to this client.
    pub events_received: u64,
    /// Frames waiting to be written to this client.
    pub queue: OutboundQueue<T>,
}

impl<T> ClientRecord<T> {
    /// A fresh record for a client that just sent `hello`.
    pub fn new(hello: &Hello, connected_at: i64, event_capacity: usize) -> Self {
        ClientRecord {
            client_id: hello.client_id.clone(),
            client_type: hello.client_type.clone(),
            connected_at,
            events_published: 0,
            events_received: 0,
            queue: OutboundQueue::new(event_capacity),
        }
    }

    /// The `StatusResp` entry for this client.
    pub fn info(&self, subscriptions: Vec<String>) -> ClientInfo {
        ClientInfo {
            client_id: self.client_id.clone(),
            client_type: self.client_type.clone(),
            connected_at: self.connected_at,
            subscriptions,
            events_published: self.events_published,
            events_received: self.events_received,
            events_dropped: self.queue.dropped(),
        }
    }
}

/// Every connection that completed Hello.
pub type Registry<T> = BTreeMap<ConnId, ClientRecord<T>>;

#[cfg(test)]
#[path = "../../tests-rs/test_bus_registry.rs"]
mod tests;
