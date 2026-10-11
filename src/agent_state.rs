//! Per-pane cache of the OSC 7501 program status a pane's agent reports.
//!
//! The server tick samples each pane's parser once per 100 ms and copies the
//! result here, so format variables never lock the parser.  A state held for
//! less than one tick can be missed.

use vt100::{BlockedKind, ProgramState, ProgramStatusRecord, Screen};

/// Longest `title`/`msg` kept for display outside the grid, in characters.
pub const MAX_TEXT_CHARS: usize = 200;

/// A change of a pane's rolled-up (worst-of) agent state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentTransition {
    /// State before the change; `None` = no record.
    pub old: Option<ProgramState>,
    /// State after the change; `None` = no record.
    pub new: Option<ProgramState>,
    /// Id of the record that now carries the worst state (`None` = root).
    pub record_id: Option<String>,
}

/// The fields of the record that carries a pane's worst-of state.  Text is
/// sanitised for the status line, `display-message` and toasts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentRecordDetails {
    /// Why the record is blocked; only set on blocked records.
    pub kind: Option<BlockedKind>,
    /// Progress 0-100.
    pub progress: Option<u8>,
    /// App name, inherited from the nearest ancestor record.
    pub app: String,
    /// Sanitised title.
    pub title: String,
    /// Sanitised one-line message.
    pub msg: String,
}

/// What the server last read from a pane's OSC 7501 records.
#[derive(Debug, Default)]
pub struct AgentStateCache {
    generation: u64,
    observed: Option<(u64, u64)>,
    root: Option<ProgramState>,
    worst: Option<ProgramState>,
    worst_id: Option<String>,
    details: AgentRecordDetails,
    prev_worst: Option<ProgramState>,
}

impl AgentStateCache {
    /// State of the root record (reported without an `id`).
    pub fn root(&self) -> Option<ProgramState> {
        self.root
    }

    /// Worst state over all records: blocked > error > working > done > idle.
    pub fn worst(&self) -> Option<ProgramState> {
        self.worst
    }

    /// Fields of the worst-of record; all empty when there is no record.
    pub fn details(&self) -> &AgentRecordDetails {
        &self.details
    }

    /// Call when the pane's parser is replaced (respawn, clear-history).
    /// A new parser restarts its sequence counter at 0, which can climb back
    /// to the cached value, so the generation makes the next sample differ.
    pub fn parser_replaced(&mut self) {
        self.generation += 1;
        self.root = None;
        self.worst = None;
        self.worst_id = None;
        self.details = AgentRecordDetails::default();
    }

    /// Samples `screen` if its records changed since the last call and
    /// returns the worst-of transition, if any.  Progress-only updates
    /// change the sequence but not the state, so they are not transitions.
    pub fn observe(&mut self, screen: &Screen) -> Option<AgentTransition> {
        if self.observed != Some((self.generation, screen.program_status_seq())) {
            self.refresh(screen);
        }
        if self.worst == self.prev_worst {
            return None;
        }
        let old = std::mem::replace(&mut self.prev_worst, self.worst);
        Some(AgentTransition {
            old,
            new: self.worst,
            record_id: self.worst_id.clone(),
        })
    }

    /// Copies `screen`'s records even when the sequence did not change, as
    /// after the process-exit lifetime rule.  Leaves transition detection to
    /// the next [`observe`](Self::observe), so a kept dead pane still reports
    /// its final transition there.
    pub fn refresh(&mut self, screen: &Screen) {
        self.observed = Some((self.generation, screen.program_status_seq()));
        self.root = screen.program_status().map(ProgramStatusRecord::state);
        let worst = worst_record(screen);
        self.worst = worst.map(|(_, r)| r.state());
        self.worst_id = worst.and_then(|(id, _)| id.map(str::to_owned));
        self.details = worst
            .map(|(id, r)| record_details(screen, id, r))
            .unwrap_or_default();
    }
}

fn worst_record(screen: &Screen) -> Option<(Option<&str>, &ProgramStatusRecord)> {
    let mut worst: Option<(Option<&str>, &ProgramStatusRecord)> = None;
    for (id, record) in screen.program_status_records() {
        if worst.is_none_or(|(_, w)| rank(record.state()) > rank(w.state())) {
            worst = Some((id, record));
        }
    }
    worst
}

fn record_details(
    screen: &Screen,
    id: Option<&str>,
    record: &ProgramStatusRecord,
) -> AgentRecordDetails {
    AgentRecordDetails {
        kind: record.kind(),
        progress: record.progress(),
        app: sanitize_text(screen.program_status_app(id).unwrap_or_default()),
        title: sanitize_text(record.title().unwrap_or_default()),
        msg: sanitize_text(record.msg().unwrap_or_default()),
    }
}

fn rank(state: ProgramState) -> u8 {
    match state {
        ProgramState::Idle => 0,
        ProgramState::Done => 1,
        ProgramState::Working => 2,
        ProgramState::Error => 3,
        ProgramState::Blocked => 4,
    }
}

/// Removes bidi overrides and invisible format characters and caps the
/// length at [`MAX_TEXT_CHARS`].  The parser already rejects control
/// characters, so text shown outside the grid cannot reorder or hide itself.
pub fn sanitize_text(text: &str) -> String {
    text.chars()
        .filter(|&c| !is_hidden_format_char(c))
        .take(MAX_TEXT_CHARS)
        .collect()
}

fn is_hidden_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
    )
}

/// The `#{pane_agent_state}` text for a state; empty when there is no record.
pub fn state_label(state: Option<ProgramState>) -> &'static str {
    match state {
        None => "",
        Some(ProgramState::Idle) => "idle",
        Some(ProgramState::Working) => "working",
        Some(ProgramState::Done) => "done",
        Some(ProgramState::Blocked) => "blocked",
        Some(ProgramState::Error) => "error",
    }
}

/// The `#{pane_agent_kind}` text; empty unless the record is blocked with a kind.
pub fn kind_label(kind: Option<BlockedKind>) -> &'static str {
    match kind {
        None => "",
        Some(BlockedKind::Permission) => "permission",
        Some(BlockedKind::Question) => "question",
        Some(BlockedKind::Auth) => "auth",
    }
}

#[cfg(test)]
#[path = "../tests-rs/test_agent_state.rs"]
mod tests;
