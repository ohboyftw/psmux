//! `wait-pane --until`, `wait-for --until` and the JSON-RPC `wait_for`
//! `state` condition: wait for a pane's agent state.
//!
//! Waiters sit in a server queue that the 100 ms tick answers after it has
//! sampled OSC 7501 state and pruned exited panes, so every caller gets the
//! same verdict from the same rule.  The rule:
//!
//! - `idle` is `--ready`: [`crate::agent_state::PaneReadiness::is_ready`],
//!   a reported idle or 500 ms of output silence.  Level-triggered.
//! - `working`, `blocked`, `done` and `error` are reported state only: the
//!   pane's worst-of OSC 7501 record.  Output alone never satisfies
//!   `working`, so a short-lived command that prints and dies is not working.
//! - A dead pane (kept by remain-on-exit) can only satisfy `done` or `error`,
//!   the states that survive exit.  Anything else on a dead or pruned pane
//!   answers [`UNTIL_GONE`].

use std::sync::mpsc;
use std::time::{Duration, Instant};

use vt100::ProgramState;

use crate::types::CtrlReq;
use crate::wait_for::WaitOutcome;

/// The wanted state was seen.
pub const UNTIL_MET: i32 = 0;
/// The deadline passed first.
pub const UNTIL_TIMEOUT: i32 = 1;
/// The pane is dead or gone (or the server stopped) before the state was seen.
pub const UNTIL_GONE: i32 = 2;

/// A queued `--until` waiter.
pub struct UntilWaiter {
    /// The `%N` pane id being watched.
    pub pane_id: usize,
    /// The state that ends the wait.
    pub want: ProgramState,
    /// When to answer [`UNTIL_TIMEOUT`]; `None` waits forever.
    pub deadline: Option<Instant>,
    /// Receives [`UNTIL_MET`], [`UNTIL_TIMEOUT`] or [`UNTIL_GONE`].
    pub resp: mpsc::Sender<i32>,
}

/// What one check sees of a waiter's pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UntilView {
    /// The pane is dead (remain-on-exit keeps it in the tree).
    pub dead: bool,
    /// `Pane::readiness(now).state`.
    pub readiness: Option<ProgramState>,
    /// The worst-of reported state, `AgentStateCache::worst`.
    pub worst: Option<ProgramState>,
}

/// The waiter's answer for one check, or `None` to keep waiting.
/// `view` is `None` when the pane no longer exists.
pub fn until_verdict(
    want: ProgramState,
    view: Option<UntilView>,
    now: Instant,
    deadline: Option<Instant>,
) -> Option<i32> {
    let Some(view) = view else {
        return Some(UNTIL_GONE);
    };
    if view.dead {
        let survives = matches!(want, ProgramState::Done | ProgramState::Error);
        return Some(if survives && view.worst == Some(want) {
            UNTIL_MET
        } else {
            UNTIL_GONE
        });
    }
    if state_matches(want, view) {
        return Some(UNTIL_MET);
    }
    match deadline {
        Some(dl) if now >= dl => Some(UNTIL_TIMEOUT),
        _ => None,
    }
}

fn state_matches(want: ProgramState, view: UntilView) -> bool {
    match want {
        ProgramState::Idle => view.readiness == Some(ProgramState::Idle),
        _ => view.worst == Some(want),
    }
}

/// Queues a waiter on the server and blocks until it is answered.
/// `pane_id` of `None` watches the active pane.  Returns [`UNTIL_MET`],
/// [`UNTIL_TIMEOUT`] or [`UNTIL_GONE`].
pub fn wait_until(
    tx: &mpsc::Sender<CtrlReq>,
    pane_id: Option<usize>,
    want: ProgramState,
    timeout_ms: Option<u64>,
) -> i32 {
    let (resp, rx) = mpsc::channel();
    let deadline = timeout_ms.map(|ms| Instant::now() + Duration::from_millis(ms));
    let req = CtrlReq::WaitPaneUntil {
        pane_id,
        want,
        deadline,
        resp,
    };
    if tx.send(req).is_err() {
        return UNTIL_GONE;
    }
    rx.recv().unwrap_or(UNTIL_GONE)
}

/// [`wait_until`] with a `timeout_ms` ceiling, as a `wait-for` outcome.
pub fn wait_state(
    tx: &mpsc::Sender<CtrlReq>,
    pane_id: Option<usize>,
    want: ProgramState,
    timeout_ms: u64,
) -> WaitOutcome {
    let start = Instant::now();
    let code = wait_until(tx, pane_id, want, Some(timeout_ms));
    until_outcome(code, start.elapsed().as_millis() as u64)
}

fn until_outcome(code: i32, elapsed_ms: u64) -> WaitOutcome {
    match code {
        UNTIL_MET => WaitOutcome::Success { elapsed_ms },
        UNTIL_TIMEOUT => WaitOutcome::Timeout { elapsed_ms },
        _ => WaitOutcome::Error {
            reason: "pane is dead or gone".into(),
        },
    }
}

#[cfg(test)]
#[path = "../tests-rs/test_wait_until.rs"]
mod tests;
