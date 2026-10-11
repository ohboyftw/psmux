//! In-tree mycel event bus: wire protocol and the data structures the bus
//! process is built from.
//!
//! Wire-compatible with the C++ mycel server for the subset psmux and the
//! existing clients use, minus its known defects (bounded subscriber
//! queues, MQTT `$`-topic rule, validated topics, wall-clock TTL).

pub mod codec;
pub mod proto;
pub mod queue;
pub mod registry;
pub mod ring;
pub mod subs;
pub mod topic;
