//! Live subscription table.
//!
//! Unlike the C++ trie: subscribing to the same pattern twice is one
//! subscription, unsubscribe removes only the named pattern, and a
//! connection with overlapping patterns receives each event once.
//! Patterns are matched with [`super::topic::matches`]; validate them
//! before they reach this table.

use super::topic;

/// Identifies one bus connection.
pub type ConnId = u64;

/// Every live `(connection, pattern)` subscription.
#[derive(Debug, Default)]
pub struct Subscriptions {
    entries: Vec<(ConnId, String)>,
}

impl Subscriptions {
    /// Add a subscription. Returns `false` if it already existed.
    pub fn subscribe(&mut self, conn: ConnId, pattern: &str) -> bool {
        if self.contains(conn, pattern) {
            return false;
        }
        self.entries.push((conn, pattern.to_owned()));
        true
    }

    /// Remove one subscription. Returns `false` if it did not exist.
    pub fn unsubscribe(&mut self, conn: ConnId, pattern: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(c, p)| !(*c == conn && p == pattern));
        self.entries.len() != before
    }

    /// Remove every subscription of a connection.
    pub fn remove_conn(&mut self, conn: ConnId) {
        self.entries.retain(|(c, _)| *c != conn);
    }

    /// Patterns a connection is subscribed to, in subscribe order.
    pub fn patterns(&self, conn: ConnId) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(c, _)| *c == conn)
            .map(|(_, p)| p.clone())
            .collect()
    }

    /// Connections with at least one pattern matching `topic`, each once,
    /// in ascending order.
    pub fn matching(&self, topic: &str) -> Vec<ConnId> {
        let mut conns: Vec<ConnId> = self
            .entries
            .iter()
            .filter(|(_, p)| topic::matches(p, topic))
            .map(|(c, _)| *c)
            .collect();
        conns.sort_unstable();
        conns.dedup();
        conns
    }

    fn contains(&self, conn: ConnId, pattern: &str) -> bool {
        self.entries.iter().any(|(c, p)| *c == conn && p == pattern)
    }
}

#[cfg(test)]
#[path = "../../tests-rs/test_bus_subs.rs"]
mod tests;
