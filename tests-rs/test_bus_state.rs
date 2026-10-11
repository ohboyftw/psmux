// Request handling tests for src/bus/state.rs (no sockets).
// Pins the O1 reply-is-next-frame rule, presence events, replay without a
// gap, bounded queues and the overflow storm guard.

use super::*;
use crate::bus::proto::{Replay, Shutdown, StatusReq, Subscribe, Unsubscribe};
use prost::Message;

const T0: i64 = 1_000_000;

fn state() -> State {
    State::new(Limits::default())
}

fn hello(s: &mut State, conn: ConnId, id: &str) {
    let msg = Msg::Hello(Hello {
        client_id: id.into(),
        client_type: "both".into(),
    });
    s.on_message(conn, msg, T0);
}

fn publish(s: &mut State, conn: ConnId, topic: &str, payload: &str, now: i64) {
    let msg = Msg::Publish(Publish {
        topic: topic.into(),
        payload: payload.as_bytes().to_vec(),
        ts_unix_ms: 0,
        metadata: HashMap::new(),
    });
    s.on_message(conn, msg, now);
}

fn subscribe(s: &mut State, conn: ConnId, pattern: &str) {
    let msg = Msg::Subscribe(Subscribe {
        topic_pattern: pattern.into(),
    });
    s.on_message(conn, msg, T0);
}

fn replay(s: &mut State, conn: ConnId, pattern: &str) {
    let msg = Msg::Replay(Replay {
        topic_pattern: pattern.into(),
        since_unix_ms: 0,
        max_events: 0,
    });
    s.on_message(conn, msg, T0);
}

/// Every message queued for `conn`, decoded, in write order.
fn drain(s: &mut State, conn: ConnId) -> Vec<Msg> {
    let mut out = Vec::new();
    while let Pop::Frame(f) = s.pop(conn) {
        let env = Envelope::decode(&f.bytes[4..]).expect("valid frame");
        out.push(env.msg.expect("message"));
    }
    out
}

fn is_ack(m: &Msg, r: &str) -> bool {
    matches!(m, Msg::Ack(a) if a.r#ref == r)
}

fn event_topics(msgs: &[Msg]) -> Vec<String> {
    msgs.iter()
        .filter_map(|m| match m {
            Msg::Event(e) => Some(e.topic.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn hello_is_acked_first() {
    let mut s = state();
    hello(&mut s, 1, "a");
    let msgs = drain(&mut s, 1);
    assert_eq!(msgs.len(), 1);
    assert!(is_ack(&msgs[0], "hello"));
}

#[test]
fn requests_before_hello_get_no_reply_and_no_effect() {
    let mut s = state();
    publish(&mut s, 1, "a/b", "x", T0);
    subscribe(&mut s, 1, "#");
    s.on_message(1, Msg::StatusReq(StatusReq {}), T0);
    assert!(matches!(s.pop(1), Pop::Gone));
    assert_eq!(s.status(T0).total_events, 0);
}

#[test]
fn hello_announces_connected_to_dollar_subscribers() {
    let mut s = state();
    hello(&mut s, 1, "monitor");
    subscribe(&mut s, 1, "$mycel/#");
    drain(&mut s, 1);
    hello(&mut s, 2, "worker");
    let msgs = drain(&mut s, 1);
    assert_eq!(event_topics(&msgs), ["$mycel/clients/connected"]);
    let Msg::Event(e) = &msgs[0] else {
        unreachable!()
    };
    assert_eq!(e.publisher, SERVER_PUBLISHER);
    let v: serde_json::Value = serde_json::from_slice(&e.payload).unwrap();
    assert_eq!(v["client_id"], "worker");
    assert_eq!(v["type"], "both");
}

#[test]
fn root_wildcards_do_not_see_presence_events() {
    let mut s = state();
    hello(&mut s, 1, "watcher");
    subscribe(&mut s, 1, "#");
    subscribe(&mut s, 1, "+/clients/connected");
    drain(&mut s, 1);
    hello(&mut s, 2, "other");
    assert!(event_topics(&drain(&mut s, 1)).is_empty());
}

#[test]
fn publish_delivers_with_sequence_publisher_and_metadata() {
    let mut s = state();
    hello(&mut s, 1, "sub");
    hello(&mut s, 2, "pub");
    subscribe(&mut s, 1, "test/#");
    drain(&mut s, 1);
    let mut metadata = HashMap::new();
    metadata.insert("k".to_owned(), "v".to_owned());
    let msg = Msg::Publish(Publish {
        topic: "test/hello".into(),
        payload: b"world".to_vec(),
        ts_unix_ms: 5,
        metadata,
    });
    s.on_message(2, msg, T0);
    let msgs = drain(&mut s, 1);
    let Msg::Event(e) = &msgs[0] else {
        panic!("expected event")
    };
    assert_eq!((e.sequence, e.publisher.as_str()), (1, "pub"));
    assert_eq!((e.payload.as_slice(), e.ts_unix_ms), (&b"world"[..], T0));
    assert_eq!(e.metadata["k"], "v");
}

#[test]
fn sequence_is_per_topic_and_shared_across_publishers() {
    let mut s = state();
    hello(&mut s, 1, "sub");
    hello(&mut s, 2, "p1");
    hello(&mut s, 3, "p2");
    subscribe(&mut s, 1, "#");
    drain(&mut s, 1);
    publish(&mut s, 2, "a", "", T0);
    publish(&mut s, 3, "a", "", T0);
    publish(&mut s, 3, "b", "", T0);
    let seqs: Vec<(String, u64)> = drain(&mut s, 1)
        .into_iter()
        .filter_map(|m| match m {
            Msg::Event(e) => Some((e.topic, e.sequence)),
            _ => None,
        })
        .collect();
    let expected = [("a", 1), ("a", 2), ("b", 1)].map(|(t, n)| (t.to_owned(), n));
    assert_eq!(seqs, expected);
}

#[test]
fn invalid_publish_topic_is_dropped_without_reply() {
    let mut s = state();
    hello(&mut s, 1, "p");
    drain(&mut s, 1);
    publish(&mut s, 1, "a/+", "x", T0);
    publish(&mut s, 1, "", "x", T0);
    assert!(drain(&mut s, 1).is_empty());
    assert!(s
        .status(T0)
        .topics
        .iter()
        .all(|t| !t.topic.starts_with('a')));
}

#[test]
fn trailing_slash_topic_shares_history_with_canonical_topic() {
    let mut s = state();
    hello(&mut s, 1, "p");
    publish(&mut s, 1, "a/b/", "1", T0);
    publish(&mut s, 1, "a/b", "2", T0);
    drain(&mut s, 1);
    replay(&mut s, 1, "a/b");
    let msgs = drain(&mut s, 1);
    assert_eq!(event_topics(&msgs), ["a/b", "a/b"]);
}

#[test]
fn subscribe_ack_precedes_events_from_the_new_subscription() {
    // O1: the Ack must be the very next frame after the request.
    let mut s = state();
    hello(&mut s, 1, "sub");
    hello(&mut s, 2, "pub");
    drain(&mut s, 1);
    subscribe(&mut s, 1, "x/#");
    publish(&mut s, 2, "x/1", "", T0);
    let msgs = drain(&mut s, 1);
    assert!(is_ack(&msgs[0], "subscribe"));
    assert_eq!(event_topics(&msgs), ["x/1"]);
}

#[test]
fn status_is_the_next_frame_after_status_req() {
    let mut s = state();
    hello(&mut s, 1, "c");
    drain(&mut s, 1);
    s.on_message(1, Msg::StatusReq(StatusReq {}), T0);
    let msgs = drain(&mut s, 1);
    assert_eq!(msgs.len(), 1);
    let Msg::StatusResp(st) = &msgs[0] else {
        panic!("expected status")
    };
    assert_eq!(st.clients[0].client_id, "c");
}

#[test]
fn invalid_subscribe_pattern_gets_error_reply() {
    let mut s = state();
    hello(&mut s, 1, "c");
    drain(&mut s, 1);
    subscribe(&mut s, 1, "a/#/b");
    let msgs = drain(&mut s, 1);
    assert!(matches!(&msgs[..], [Msg::Error(_)]));
}

#[test]
fn unsubscribe_removes_only_the_named_pattern() {
    let mut s = state();
    hello(&mut s, 1, "sub");
    hello(&mut s, 2, "pub");
    subscribe(&mut s, 1, "a");
    subscribe(&mut s, 1, "b");
    let unsub = Msg::Unsubscribe(Unsubscribe {
        topic_pattern: "a".into(),
    });
    s.on_message(1, unsub, T0);
    drain(&mut s, 1);
    publish(&mut s, 2, "a", "", T0);
    publish(&mut s, 2, "b", "", T0);
    assert_eq!(event_topics(&drain(&mut s, 1)), ["b"]);
}

#[test]
fn replay_sends_events_then_replay_complete() {
    let mut s = state();
    hello(&mut s, 1, "p");
    for i in 0..3 {
        publish(&mut s, 1, "replay/x", &i.to_string(), T0 + i);
    }
    drain(&mut s, 1);
    replay(&mut s, 1, "replay/#");
    let msgs = drain(&mut s, 1);
    assert_eq!(msgs.len(), 4);
    assert!(is_ack(&msgs[3], "replay_complete"));
}

#[test]
fn replay_with_invalid_pattern_still_completes() {
    let mut s = state();
    hello(&mut s, 1, "p");
    drain(&mut s, 1);
    replay(&mut s, 1, "a+/b");
    let msgs = drain(&mut s, 1);
    assert!(matches!(&msgs[..], [m] if is_ack(m, "replay_complete")));
}

#[test]
fn replay_then_subscribe_delivers_events_published_in_between() {
    let mut s = state();
    hello(&mut s, 1, "late");
    hello(&mut s, 2, "pub");
    publish(&mut s, 2, "g/1", "before", T0);
    replay(&mut s, 1, "g/#");
    publish(&mut s, 2, "g/1", "gap", T0 + 1);
    publish(&mut s, 2, "g/2", "gap-new-topic", T0 + 2);
    drain(&mut s, 1);
    subscribe(&mut s, 1, "g/#");
    let msgs = drain(&mut s, 1);
    assert!(is_ack(&msgs[0], "subscribe"));
    let payloads: Vec<Vec<u8>> = msgs[1..]
        .iter()
        .filter_map(|m| match m {
            Msg::Event(e) => Some(e.payload.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(payloads, [b"gap".to_vec(), b"gap-new-topic".to_vec()]);
}

#[test]
fn replay_gap_is_delivered_only_once() {
    let mut s = state();
    hello(&mut s, 1, "late");
    hello(&mut s, 2, "pub");
    replay(&mut s, 1, "g/#");
    publish(&mut s, 2, "g/1", "gap", T0);
    subscribe(&mut s, 1, "g/#");
    let unsub = Msg::Unsubscribe(Unsubscribe {
        topic_pattern: "g/#".into(),
    });
    s.on_message(1, unsub, T0);
    drain(&mut s, 1);
    subscribe(&mut s, 1, "g/#");
    assert!(event_topics(&drain(&mut s, 1)).is_empty());
}

#[test]
fn slow_subscriber_drops_oldest_events_but_keeps_acks() {
    let limits = Limits {
        event_capacity: 2,
        ..Limits::default()
    };
    let mut s = State::new(limits);
    hello(&mut s, 1, "slow");
    hello(&mut s, 2, "pub");
    subscribe(&mut s, 1, "#");
    for i in 0..5 {
        publish(&mut s, 2, "t", &i.to_string(), T0);
    }
    s.on_message(1, Msg::StatusReq(StatusReq {}), T0);
    let msgs = drain(&mut s, 1);
    assert!(is_ack(&msgs[0], "hello") && is_ack(&msgs[1], "subscribe"));
    assert_eq!(event_topics(&msgs).len(), 2);
    let Some(Msg::StatusResp(st)) = msgs.last() else {
        panic!("status last")
    };
    assert_eq!(st.clients[0].events_dropped, 3);
}

#[test]
fn disconnect_announces_once() {
    let mut s = state();
    hello(&mut s, 1, "monitor");
    subscribe(&mut s, 1, "$mycel/clients/disconnected");
    hello(&mut s, 2, "gone");
    drain(&mut s, 1);
    assert!(s.disconnect(2, T0));
    assert!(!s.disconnect(2, T0));
    assert_eq!(event_topics(&drain(&mut s, 1)).len(), 1);
}

#[test]
fn disconnect_before_hello_announces_nothing() {
    let mut s = state();
    assert!(!s.disconnect(7, T0));
    assert_eq!(s.status(T0).total_events, 0);
}

#[test]
fn shutdown_is_acked_with_the_stop_flag() {
    let mut s = state();
    hello(&mut s, 1, "cli");
    drain(&mut s, 1);
    let msg = Msg::Shutdown(Shutdown {
        reason: "test".into(),
    });
    assert!(matches!(s.on_message(1, msg, T0), Action::Shutdown));
    let Pop::Frame(f) = s.pop(1) else {
        panic!("ack queued")
    };
    assert!(f.stop_bus);
}

#[test]
fn max_connections_rejects_the_next_hello() {
    let limits = Limits {
        max_connections: 1,
        ..Limits::default()
    };
    let mut s = State::new(limits);
    hello(&mut s, 1, "first");
    let msg = Msg::Hello(Hello::default());
    let Action::Reject(bytes) = s.on_message(2, msg, T0) else {
        panic!("rejected")
    };
    let env = Envelope::decode(&bytes[4..]).unwrap();
    assert!(matches!(env.msg, Some(Msg::Error(e)) if e.message == "Server at max connections"));
}

#[test]
fn overflow_is_announced_without_recursing() {
    let limits = Limits {
        ring: RingConfig {
            max_events: 1,
            ttl_ms: 0,
        },
        ..Limits::default()
    };
    let mut s = State::new(limits);
    hello(&mut s, 1, "p");
    subscribe(&mut s, 1, "$mycel/server/#");
    for _ in 0..50 {
        publish(&mut s, 1, "busy", "", T0);
    }
    let overflows = event_topics(&drain(&mut s, 1))
        .into_iter()
        .filter(|t| t == "$mycel/server/buffer_overflow")
        .count();
    assert_eq!(overflows, 49);
}

#[test]
fn status_counts_subscribers_per_topic() {
    let mut s = state();
    hello(&mut s, 1, "a");
    hello(&mut s, 2, "b");
    subscribe(&mut s, 1, "t/#");
    subscribe(&mut s, 2, "t/x");
    publish(&mut s, 1, "t/x", "", T0);
    let st = s.status(T0);
    let t = st.topics.iter().find(|t| t.topic == "t/x").unwrap();
    assert_eq!((t.subscriber_count, t.event_count), (2, 1));
}

#[test]
fn heartbeat_publishes_one_event_per_client() {
    let mut s = state();
    hello(&mut s, 1, "a");
    hello(&mut s, 2, "b");
    subscribe(&mut s, 1, "$mycel/clients/heartbeat");
    drain(&mut s, 1);
    s.heartbeat(T0);
    assert_eq!(event_topics(&drain(&mut s, 1)).len(), 2);
}

#[test]
fn liveness_probe_announces_no_presence() {
    let mut s = state();
    hello(&mut s, 1, "monitor");
    subscribe(&mut s, 1, "$mycel/#");
    drain(&mut s, 1);
    hello(&mut s, 2, crate::bus::client::PROBE_CLIENT_ID);
    s.disconnect(2, T0);
    assert!(event_topics(&drain(&mut s, 1)).is_empty());
}
