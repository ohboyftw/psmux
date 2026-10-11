//! Which OSC 7501 transitions become desktop toasts, and when they fire.
//!
//! Only entry into `blocked`, `error` or `done` is offered.  Each pane holds
//! at most one pending toast, so a toast that cannot fire yet (another toast
//! in flight, a rate limit) waits instead of being dropped.  The policy never
//! spawns anything itself; the server tick asks [`AgentToasts::due`] and
//! reports back with [`AgentToasts::mark_fired`].

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use vt100::{BlockedKind, ProgramState};

use crate::agent_state::AgentTransition;

/// A pane that re-enters `done` or `error`, the state it last toasted,
/// within this window is not toasted again.  `blocked` is exempt: each
/// new prompt is a new question, limited only by [`PANE_MIN_INTERVAL`].
pub const DEDUPE_WINDOW: Duration = Duration::from_secs(30);
/// Shortest gap between two toasts for one pane; later ones wait.
pub const PANE_MIN_INTERVAL: Duration = Duration::from_secs(5);
/// Shortest gap between any two agent toasts.
pub const MIN_INTERVAL: Duration = Duration::from_secs(1);

/// One toast waiting to be shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    /// Pane whose agent changed state.
    pub pane_id: usize,
    /// The state that was entered.
    pub state: ProgramState,
    /// Toast heading, for example "claude-code needs permission".
    pub title: String,
    /// Toast text: session, pane and the record's message.
    pub body: String,
}

/// Toast policy for the `agent-toasts` option: dedupe, rate limits and the
/// pending slot per pane.
#[derive(Debug)]
pub struct AgentToasts {
    enabled: bool,
    pending: HashMap<usize, Toast>,
    last_toast: HashMap<usize, (ProgramState, Instant)>,
    last_fired: Option<Instant>,
    exited: HashSet<usize>,
}

impl Default for AgentToasts {
    fn default() -> Self {
        Self {
            enabled: true,
            pending: HashMap::new(),
            last_toast: HashMap::new(),
            last_fired: None,
            exited: HashSet::new(),
        }
    }
}

impl AgentToasts {
    /// Whether agent toasts are on (`agent-toasts`, default on).
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Turns agent toasts on or off.  Turning them off drops what is pending.
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            self.pending.clear();
            self.exited.clear();
        }
    }

    /// Number of toasts waiting to fire.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Considers one transition.  A pane that leaves `blocked` before its
    /// toast fired loses that toast: the question was already answered.
    pub fn offer(&mut self, pane_id: usize, t: &AgentTransition, session: &str, now: Instant) {
        if t.new != Some(ProgramState::Blocked)
            && self.pending.get(&pane_id).map(|p| p.state) == Some(ProgramState::Blocked)
        {
            self.pending.remove(&pane_id);
        }
        let Some(state) = t.new.filter(|&s| priority(s) > 0) else {
            return;
        };
        if !self.enabled || self.recently_toasted(pane_id, state, now) {
            return;
        }
        if self
            .pending
            .get(&pane_id)
            .is_some_and(|p| priority(p.state) > priority(state))
        {
            return;
        }
        self.pending
            .insert(pane_id, build_toast(pane_id, state, t, session));
    }

    /// The toast to show now, if any: the highest priority pending toast
    /// whose pane is outside its rate limit.  Ties go to the lowest pane id.
    pub fn due(&self, now: Instant) -> Option<&Toast> {
        if !self.enabled || self.last_fired.is_some_and(|t| now - t < MIN_INTERVAL) {
            return None;
        }
        self.pending
            .values()
            .filter(|toast| !self.pane_rate_limited(toast.pane_id, now))
            .max_by_key(|toast| (priority(toast.state), std::cmp::Reverse(toast.pane_id)))
    }

    /// Records that the pending toast of `pane_id` was shown.
    pub fn mark_fired(&mut self, pane_id: usize, now: Instant) {
        self.exited.remove(&pane_id);
        if let Some(toast) = self.pending.remove(&pane_id) {
            self.last_toast.insert(pane_id, (toast.state, now));
            self.last_fired = Some(now);
        }
        self.last_toast
            .retain(|_, (_, at)| now - *at < DEDUPE_WINDOW);
    }

    /// Marks a pane whose child exited on its own, so its waiting toast
    /// outlives the pane leaving the window tree.
    pub fn note_exited(&mut self, pane_id: usize) {
        if self.pending.contains_key(&pane_id) {
            self.exited.insert(pane_id);
        }
    }

    /// Drops the waiting toast of every pane that is neither in `live` nor
    /// marked by [`AgentToasts::note_exited`]: a killed pane says nothing.
    pub fn forget_gone(&mut self, live: &HashSet<usize>) {
        let exited = &self.exited;
        self.pending
            .retain(|id, _| live.contains(id) || exited.contains(id));
        let pending = &self.pending;
        self.exited.retain(|id| pending.contains_key(id));
    }

    /// Removes and returns the most important waiting toast, ignoring every
    /// rate limit.  For the last chance before the server exits.
    pub fn take_any(&mut self) -> Option<Toast> {
        if !self.enabled {
            return None;
        }
        let id = self
            .pending
            .values()
            .max_by_key(|toast| (priority(toast.state), std::cmp::Reverse(toast.pane_id)))?
            .pane_id;
        self.exited.remove(&id);
        self.pending.remove(&id)
    }

    fn recently_toasted(&self, pane_id: usize, state: ProgramState, now: Instant) -> bool {
        state != ProgramState::Blocked
            && self
                .last_toast
                .get(&pane_id)
                .is_some_and(|&(s, at)| s == state && now - at < DEDUPE_WINDOW)
    }

    fn pane_rate_limited(&self, pane_id: usize, now: Instant) -> bool {
        self.last_toast
            .get(&pane_id)
            .is_some_and(|&(_, at)| now - at < PANE_MIN_INTERVAL)
    }
}

/// blocked > error > done; 0 means the state is never toasted.
fn priority(state: ProgramState) -> u8 {
    match state {
        ProgramState::Blocked => 3,
        ProgramState::Error => 2,
        ProgramState::Done => 1,
        ProgramState::Working | ProgramState::Idle => 0,
    }
}

fn build_toast(pane_id: usize, state: ProgramState, t: &AgentTransition, session: &str) -> Toast {
    let d = &t.details;
    let app = if d.app.is_empty() { "Agent" } else { &d.app };
    let text = if d.msg.is_empty() { &d.title } else { &d.msg };
    let place = format!("{session} %{pane_id}");
    Toast {
        pane_id,
        state,
        title: format!("{app} {}", verb(state, d.kind)),
        body: if text.is_empty() {
            place
        } else {
            format!("{place}: {text}")
        },
    }
}

fn verb(state: ProgramState, kind: Option<BlockedKind>) -> &'static str {
    match (state, kind) {
        (ProgramState::Blocked, Some(BlockedKind::Permission)) => "needs permission",
        (ProgramState::Blocked, Some(BlockedKind::Question)) => "has a question",
        (ProgramState::Blocked, Some(BlockedKind::Auth)) => "needs sign-in",
        (ProgramState::Blocked, None) => "is blocked",
        (ProgramState::Error, _) => "reported an error",
        _ => "is done",
    }
}

#[cfg(test)]
#[path = "../tests-rs/test_agent_toast.rs"]
mod test_agent_toast;
