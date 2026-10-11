// Outbound queue tests for src/bus/queue.rs.
// mycel tests/test_subscriber.cpp SendBufferTest cases, re-targeted at the
// real per-connection queue, plus spec 12.6 #13.

use super::*;

fn drain(q: &mut OutboundQueue<u32>) -> Vec<u32> {
    std::iter::from_fn(|| q.pop()).collect()
}

#[test]
fn enqueue_dequeue_is_fifo() {
    let mut q = OutboundQueue::new(10);
    q.push_event(1);
    q.push_event(2);
    assert_eq!(drain(&mut q), [1, 2]);
}

#[test]
fn dequeue_empty_returns_none() {
    let mut q: OutboundQueue<u32> = OutboundQueue::new(10);
    assert_eq!(q.pop(), None);
    assert!(q.is_empty());
}

#[test]
fn backpressure_drops_oldest_event() {
    let mut q = OutboundQueue::new(3);
    for i in 1..=3 {
        assert!(!q.push_event(i));
    }
    assert!(q.push_event(4));
    assert_eq!(drain(&mut q), [2, 3, 4]);
}

#[test]
fn dropped_count_accumulates() {
    let mut q = OutboundQueue::new(2);
    for i in 1..=7 {
        q.push_event(i);
    }
    assert_eq!(q.dropped(), 5);
    assert_eq!(q.len(), 2);
}

#[test]
fn control_frames_are_never_dropped() {
    let mut q = OutboundQueue::new(2);
    q.push_event(1);
    q.push_control(100);
    for i in 2..=5 {
        q.push_event(i);
    }
    assert_eq!(drain(&mut q), [100, 4, 5]);
    assert_eq!(q.dropped(), 3);
}

#[test]
fn control_frames_do_not_use_event_capacity() {
    let mut q = OutboundQueue::new(1);
    for c in 100..110 {
        q.push_control(c);
    }
    assert!(!q.push_event(1));
    assert_eq!(q.len(), 11);
}

#[test]
fn slow_subscriber_stays_bounded_and_keeps_ack_order() {
    let mut q = OutboundQueue::new(DEFAULT_EVENT_CAPACITY);
    for i in 0..1500 {
        q.push_event(i);
    }
    q.push_control(u32::MAX);
    for i in 1500..2000 {
        q.push_event(i);
    }
    assert_eq!(q.len(), DEFAULT_EVENT_CAPACITY + 1);
    assert_eq!(q.dropped(), 1000);
    let out = drain(&mut q);
    assert_eq!(out.iter().filter(|f| **f == u32::MAX).count(), 1);
    let events: Vec<u32> = out.into_iter().filter(|f| *f != u32::MAX).collect();
    assert_eq!(events, (1000..2000).collect::<Vec<_>>());
}

#[test]
fn reply_after_events_stays_behind_them() {
    let mut q = OutboundQueue::new(10);
    q.push_event(1);
    q.push_control(100);
    q.push_event(2);
    assert_eq!(drain(&mut q), [1, 100, 2]);
}

#[test]
fn zero_capacity_drops_every_event() {
    let mut q = OutboundQueue::new(0);
    assert!(q.push_event(1));
    q.push_control(2);
    assert_eq!((drain(&mut q), q.dropped()), (vec![2], 1));
}

#[test]
fn queued_control_counts_only_undroppable_frames() {
    let mut q = OutboundQueue::new(10);
    q.push_event(1);
    q.push_control(2);
    q.push_control(3);
    assert_eq!((q.queued_control(), q.len()), (2, 3));
}
