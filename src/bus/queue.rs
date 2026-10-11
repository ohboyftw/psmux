//! Bounded outbound queue for one connection.
//!
//! Live events are droppable: when the queue already holds its limit of
//! events, the oldest queued *event* is dropped and counted. Control frames
//! (Ack, StatusResp, Error) and replay frames are never dropped and do not
//! count toward the limit, so a request's reply always arrives and stays
//! the very next frame after anything queued before it. Order is FIFO.
//!
//! The C++ server's equivalent held at most one frame and forwarded it to
//! an unbounded write queue, so it never dropped anything.

use std::collections::VecDeque;

/// Default live-event limit per connection.
pub const DEFAULT_EVENT_CAPACITY: usize = 1000;

#[derive(Debug)]
struct Entry<T> {
    frame: T,
    droppable: bool,
}

/// FIFO of frames waiting to be written to one connection.
#[derive(Debug)]
pub struct OutboundQueue<T> {
    entries: VecDeque<Entry<T>>,
    event_capacity: usize,
    queued_events: usize,
    dropped: u64,
}

impl<T> OutboundQueue<T> {
    /// An empty queue that holds at most `event_capacity` live events.
    pub fn new(event_capacity: usize) -> Self {
        OutboundQueue {
            entries: VecDeque::new(),
            event_capacity,
            queued_events: 0,
            dropped: 0,
        }
    }

    /// Queue a live event, dropping the oldest queued event when full.
    /// Returns `true` if an event was dropped to make room.
    pub fn push_event(&mut self, frame: T) -> bool {
        let dropped = self.queued_events >= self.event_capacity && self.drop_oldest_event();
        self.entries.push_back(Entry {
            frame,
            droppable: true,
        });
        self.queued_events += 1;
        dropped
    }

    /// Queue a frame that must never be dropped.
    pub fn push_control(&mut self, frame: T) {
        self.entries.push_back(Entry {
            frame,
            droppable: false,
        });
    }

    /// Take the next frame to write.
    pub fn pop(&mut self) -> Option<T> {
        let entry = self.entries.pop_front()?;
        if entry.droppable {
            self.queued_events -= 1;
        }
        Some(entry.frame)
    }

    /// Frames waiting, of either kind.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when nothing is waiting.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Live events dropped since the queue was created.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    fn drop_oldest_event(&mut self) -> bool {
        let Some(i) = self.entries.iter().position(|e| e.droppable) else {
            return false;
        };
        self.entries.remove(i);
        self.queued_events -= 1;
        self.dropped += 1;
        true
    }
}

#[cfg(test)]
#[path = "../../tests-rs/test_bus_queue.rs"]
mod tests;
