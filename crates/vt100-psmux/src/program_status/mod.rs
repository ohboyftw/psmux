//! OSC 7501 Program Status Protocol: report parsing and the per-terminal
//! record store.
//!
//! Spec: <https://www.superlogical.com/rex/docs/build/program-status>.

use std::collections::BTreeMap;

use parse::{parse_report, Action, Report};

mod parse;

const MAX_SEQUENCE_LEN: usize = 4096;
const MAX_RECORDS: usize = 256;

/// The exact bytes a supporting terminal answers to `OSC 7501 ; ? ST`.
pub const QUERY_REPLY: &[u8] = b"\x1b]7501;?\x1b\\";

/// The state a program reported for one record.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ProgramState {
    /// Waiting for the user's next instruction.
    Idle,
    /// Running; may carry a progress percentage.
    Working,
    /// Finished, with results the user has not seen yet.
    Done,
    /// Needs user action; may carry a [`BlockedKind`].
    Blocked,
    /// Failed and stopped.
    Error,
}

/// Why a [`ProgramState::Blocked`] record needs the user.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BlockedKind {
    /// The program is asking for permission to act.
    Permission,
    /// The program is asking the user a question.
    Question,
    /// The program needs credentials.
    Auth,
}

/// One program status record, as last reported for its id.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramStatusRecord {
    state: ProgramState,
    kind: Option<BlockedKind>,
    progress: Option<u8>,
    app: Option<String>,
    title: Option<String>,
    msg: Option<String>,
    stamp: u64,
}

impl ProgramStatusRecord {
    /// The reported state.
    #[must_use]
    pub fn state(&self) -> ProgramState {
        self.state
    }

    /// Why the program is blocked; only ever set on blocked records.
    #[must_use]
    pub fn kind(&self) -> Option<BlockedKind> {
        self.kind
    }

    /// Progress 0-100; only ever set on working or blocked records.
    #[must_use]
    pub fn progress(&self) -> Option<u8> {
        self.progress
    }

    /// The app name this record itself carries, without inheritance.
    /// Use [`crate::Screen::program_status_app`] for the inherited value.
    #[must_use]
    pub fn app(&self) -> Option<&str> {
        self.app.as_deref()
    }

    /// The decoded title, free of control characters.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The decoded one-line message, free of control characters.
    #[must_use]
    pub fn msg(&self) -> Option<&str> {
        self.msg.as_deref()
    }
}

/// All program status records of one terminal.  The root record is keyed by
/// the empty string.
#[derive(Clone, Debug, Default)]
pub struct Store {
    records: BTreeMap<String, ProgramStatusRecord>,
    seq: u64,
    seen: bool,
}

impl Store {
    /// Handles an OSC 7501 body (everything after `7501;`).  `seq_len` is the
    /// whole escape sequence length including introducer and terminator.
    /// Returns `true` when the body was the support query.
    pub(crate) fn handle_osc(&mut self, body: &[u8], seq_len: usize) -> bool {
        if body == b"?" {
            return true;
        }
        if seq_len <= MAX_SEQUENCE_LEN {
            if let Some(report) = parse_report(body) {
                self.apply(report);
            }
        }
        false
    }

    fn apply(&mut self, report: Report) {
        self.seen = true;
        match report.action {
            Action::Set(mut record) => {
                self.seq += 1;
                record.stamp = self.seq;
                if !self.records.contains_key(&report.id) && self.records.len() >= MAX_RECORDS {
                    self.evict_oldest();
                }
                self.records.insert(report.id, record);
            }
            Action::Clear => {
                let id = report.id;
                self.remove_where(|key| id.is_empty() || key == id || is_descendant(key, &id));
            }
        }
    }

    fn evict_oldest(&mut self) {
        let oldest = self
            .records
            .iter()
            .min_by_key(|(_, r)| r.stamp)
            .map(|(k, _)| k.clone());
        if let Some(key) = oldest {
            self.records.remove(&key);
        }
    }

    fn remove_where(&mut self, doomed: impl Fn(&str) -> bool) {
        let before = self.records.len();
        self.records.retain(|key, _| !doomed(key));
        if self.records.len() != before {
            self.seq += 1;
        }
    }

    /// Drops working, blocked and idle records; done and error survive.
    pub(crate) fn end_of_run(&mut self) {
        let before = self.records.len();
        self.records
            .retain(|_, r| matches!(r.state, ProgramState::Done | ProgramState::Error));
        if self.records.len() != before {
            self.seq += 1;
        }
    }

    /// Full reset (RIS): removes every record and forgets that any report was
    /// seen, keeping `seq` monotonic.
    pub(crate) fn reset(&mut self) {
        if !self.records.is_empty() || self.seen {
            self.seq += 1;
        }
        self.records.clear();
        self.seen = false;
    }

    pub(crate) fn root(&self) -> Option<&ProgramStatusRecord> {
        self.records.get("")
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (Option<&str>, &ProgramStatusRecord)> {
        self.records
            .iter()
            .map(|(k, r)| ((!k.is_empty()).then_some(k.as_str()), r))
    }

    /// The app of the record at `id`, else of its nearest ancestor (the root
    /// record is every id's ancestor).  `None` when no such record exists.
    pub(crate) fn effective_app(&self, id: Option<&str>) -> Option<&str> {
        let mut key = id.unwrap_or("");
        self.records.get(key)?;
        loop {
            if let Some(app) = self.records.get(key).and_then(|r| r.app.as_deref()) {
                return Some(app);
            }
            if key.is_empty() {
                return None;
            }
            key = key.rfind('/').map_or("", |i| &key[..i]);
        }
    }

    pub(crate) fn seq(&self) -> u64 {
        self.seq
    }

    pub(crate) fn seen(&self) -> bool {
        self.seen
    }
}

fn is_descendant(key: &str, ancestor: &str) -> bool {
    key.len() > ancestor.len()
        && key.starts_with(ancestor)
        && key.as_bytes()[ancestor.len()] == b'/'
}

#[cfg(test)]
mod tests;
