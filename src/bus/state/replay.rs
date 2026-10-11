//! Replay, and the replay-then-subscribe gap.
//!
//! A Replay remembers, per pattern, the newest sequence it returned on each
//! topic. A later Subscribe of the same pattern delivers what was
//! published in between, after its Ack.

use std::collections::HashMap;

use super::super::proto::Replay;
use super::super::subs::ConnId;
use super::super::topic;
use super::{ack, event_frame, State, MAX_MARKS_PER_CONN};

impl State {
    pub(super) fn on_replay(&mut self, conn: ConnId, r: &Replay, now: i64) {
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

    /// After a Replay then Subscribe of the same pattern, deliver what was
    /// published in between, after the Ack. Events an earlier subscription
    /// of this connection already delivered live are skipped.
    ///
    /// Known limits: "already delivered" is judged by the patterns held now,
    /// not when each event was published, so a gap event whose covering
    /// subscription was made after it was published is skipped too. Gap
    /// events are live events, so a gap larger than the connection's event
    /// capacity loses its oldest ones (counted as drops).
    pub(super) fn deliver_replay_gap(
        &mut self,
        conn: ConnId,
        pattern: &str,
        earlier: &[String],
        now: i64,
    ) {
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
}
