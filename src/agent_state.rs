//! Per-pane cache of the OSC 7501 program status a pane's agent reports.
//!
//! The server tick samples each pane's parser once per 100 ms and copies the
//! result here, so format variables never lock the parser.  A state held for
//! less than one tick can be missed.

use vt100::{ProgramState, Screen};

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

/// What the server last read from a pane's OSC 7501 records.
#[derive(Debug, Default)]
pub struct AgentStateCache {
    generation: u64,
    observed: Option<(u64, u64)>,
    root: Option<ProgramState>,
    worst: Option<ProgramState>,
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

    /// Call when the pane's parser is replaced (respawn, clear-history).
    /// A new parser restarts its sequence counter at 0, which can climb back
    /// to the cached value, so the generation makes the next sample differ.
    pub fn parser_replaced(&mut self) {
        self.generation += 1;
        self.root = None;
        self.worst = None;
    }

    /// Samples `screen` if its records changed since the last call and
    /// returns the worst-of transition, if any.  Progress-only updates
    /// change the sequence but not the state, so they are not transitions.
    pub fn observe(&mut self, screen: &Screen) -> Option<AgentTransition> {
        let key = (self.generation, screen.program_status_seq());
        if self.observed == Some(key) {
            return None;
        }
        self.observed = Some(key);
        self.root = screen
            .program_status()
            .map(vt100::ProgramStatusRecord::state);
        let worst = worst_record(screen);
        self.worst = worst.as_ref().map(|(_, s)| *s);
        if self.worst == self.prev_worst {
            return None;
        }
        let old = std::mem::replace(&mut self.prev_worst, self.worst);
        Some(AgentTransition {
            old,
            new: self.worst,
            record_id: worst.and_then(|(id, _)| id),
        })
    }
}

fn worst_record(screen: &Screen) -> Option<(Option<String>, ProgramState)> {
    let mut worst: Option<(Option<String>, ProgramState)> = None;
    for (id, record) in screen.program_status_records() {
        let state = record.state();
        if worst.as_ref().is_none_or(|(_, w)| rank(state) > rank(*w)) {
            worst = Some((id.map(str::to_owned), state));
        }
    }
    worst
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

#[cfg(test)]
#[path = "../tests-rs/test_agent_state.rs"]
mod tests;
