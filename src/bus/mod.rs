//! In-tree mycel event bus: wire protocol, the `psmux bus` process, and
//! the blocking client psmux publishes through.
//!
//! Wire-compatible with the C++ mycel server for the subset psmux and the
//! existing clients use, minus its known defects (bounded subscriber
//! queues, MQTT `$`-topic rule, validated topics, wall-clock TTL, no
//! replay-then-subscribe gap).

pub mod client;
pub mod codec;
pub mod discovery;
pub mod host;
pub mod proto;
pub mod queue;
pub mod registry;
pub mod ring;
pub mod server;
pub mod state;
pub mod subs;
pub mod topic;
